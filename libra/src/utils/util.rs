use path_abs::{PathAbs, PathInfo};
use std::collections::HashSet;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use mercury::hash::SHA1;
use mercury::internal::object::types::ObjectType;

use crate::utils::client_storage::ClientStorage;
use crate::utils::path;
use crate::utils::path_ext::PathExt;

pub const ROOT_DIR: &str = ".libra";
pub const DATABASE: &str = "libra.db";

/// Returns the current working directory as a `PathBuf`.
///
/// This function wraps the `std::env::current_dir()` function and unwraps the result.
/// If the current directory value is not available for any reason, this function will panic.
///
/// TODO - Add additional check result from `std::env::current_dir()` to handle the panic
///
/// # Returns
///
/// A `PathBuf` representing the current working directory.
pub fn cur_dir() -> PathBuf {
    env::current_dir().unwrap()
}

/// Try to get the storage path of the repository, which is the path of the `.libra` directory
/// - if the current directory is not a repository, return an error
pub fn try_get_storage_path() -> Result<PathBuf, io::Error> {
    /*递归获取储存库 */
    let mut cur_dir = env::current_dir()?;
    loop {
        let mut libra = cur_dir.clone();
        libra.push(ROOT_DIR);
        if libra.exists() {
            return Ok(libra);
        }
        if !cur_dir.pop() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{:?} is not a git repository", env::current_dir()?),
            ));
        }
    }
}

/// Get the storage path of the repository, aka `.libra`
/// - panics if the current directory is not a repository
pub fn storage_path() -> PathBuf {
    try_get_storage_path().unwrap()
}
/// Check if libra repo exists
pub fn check_repo_exist() -> bool {
    if try_get_storage_path().is_err() {
        eprintln!("fatal: not a libra repository (or any of the parent directories): .libra");
        return false;
    }
    true
}

/// Get `ClientStorage` for the `objects` directory
pub fn objects_storage() -> ClientStorage {
    ClientStorage::init(path::objects())
}

/// Get the working directory of the repository
/// - panics if the current directory is not a repository
pub fn working_dir() -> PathBuf {
    let mut storage_path = storage_path();
    storage_path.pop();
    storage_path
}

/// Get the working directory of the repository as a string, panics if the path is not valid utf-8
pub fn working_dir_string() -> String {
    working_dir().to_str().unwrap().to_string()
}

/// Turn a path to a relative path to the working directory
/// - not check existence
pub fn to_workdir_path(path: impl AsRef<Path>) -> PathBuf {
    to_relative(path, working_dir())
}

/// Turn a workdir path to absolute path
pub fn workdir_to_absolute(path: impl AsRef<Path>) -> PathBuf {
    working_dir().join(path.as_ref())
}

/// Judge if the path is a sub path of the parent path
/// - Not check existence
/// - `true` if path == parent
pub fn is_sub_path<P, B>(path: P, parent: B) -> bool
where P: AsRef<Path>, B: AsRef<Path>
{
    let path_abs = PathAbs::new(path.as_ref()).unwrap(); // prefix: '\\?\' on Windows
    let parent_abs = PathAbs::new(parent.as_ref()).unwrap();
    path_abs.starts_with(parent_abs)
}

/// Judge if the `path` is sub-path of `paths`(include sub-dirs)
/// - absolute path or relative path to the current dir
/// - Not check existence
pub fn is_sub_of_paths<P, U>(path: impl AsRef<Path>, paths: U) -> bool
where
    P: AsRef<Path>,
    U: IntoIterator<Item = P>,
{
    for p in paths {
        if is_sub_path(path.as_ref(), p.as_ref()) {
            return true;
        }
    }
    false
}

/// Filter paths to fit the given paths, include sub-dirs
/// - return the paths that are sub-path of the fit paths
/// - `paths`: to workdir
/// - `fit_paths`: abs or rel
/// - Not check existence
pub fn filter_to_fit_paths<P>(paths: &[P], fit_paths: &Vec<P>) -> Vec<P>
where
    P: AsRef<Path> + Clone,
{
    paths
        .iter()
        .filter(|p| {
            let p = workdir_to_absolute(p.as_ref());
            is_sub_of_paths(p, fit_paths)
        })
        .cloned()
        .collect()
}

/// `path` & `base` must be absolute or relative (to current dir)
pub fn to_relative<P, B>(path: P, base: B) -> PathBuf
where
    P: AsRef<Path>,
    B: AsRef<Path>,
{
    let path_abs = PathAbs::new(path.as_ref()).unwrap(); // prefix: '\\?\' on Windows
    let base_abs = PathAbs::new(base.as_ref()).unwrap();
    if cfg!(windows) {
        assert_eq!(
            // just little check
            path_abs.to_str().unwrap().starts_with(r"\\?\"),
            base_abs.to_str().unwrap().starts_with(r"\\?\")
        )
    }
    if let Some(rel_path) = pathdiff::diff_paths(path_abs, base_abs) {
        rel_path
    } else {
        panic!(
            "fatal: path {:?} cannot convert to relative based on {:?}",
            path.as_ref(),
            base.as_ref()
        );
    }
}

#[allow(dead_code)]
/// Convert a path to relative path to the current directory
/// - `path` must be absolute or relative (to current dir)
pub fn to_current_dir<P>(path: P) -> PathBuf
where
    P: AsRef<Path>,
{
    to_relative(path, cur_dir())
}

/// Convert a workdir path to relative path
/// - `base` must be absolute or relative (to current dir)
pub fn workdir_to_relative<P, B>(path: P, base: B) -> PathBuf
where
    P: AsRef<Path>,
    B: AsRef<Path>,
{
    let path_abs = workdir_to_absolute(path);
    to_relative(path_abs, base)
}

/// Convert a workdir path to relative path to the current directory
pub fn workdir_to_current<P>(path: P) -> PathBuf
where
    P: AsRef<Path>,
{
    workdir_to_relative(path, cur_dir())
}

pub fn calc_file_blob_hash(path: impl AsRef<Path>) -> io::Result<SHA1> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    Ok(SHA1::from_type_and_data(ObjectType::Blob, &data))
}

/// List all files in the given dir and its sub_dir, except `.libra`
/// - input `path`: absolute path or relative path to the current dir
/// - output: to workdir path
pub fn list_files(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if path.is_dir() {
        if path.file_name().unwrap_or_default() == ROOT_DIR {
            // ignore `.libra`
            return Ok(files);
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(list_files(&path)?);
            } else {
                files.push(to_workdir_path(&path));
            }
        }
    }
    Ok(files)
}

/// list all files in the working dir(include sub_dir)
/// - output: to workdir path
pub fn list_workdir_files() -> io::Result<Vec<PathBuf>> {
    list_files(&working_dir())
}

/// Integrate the input paths (relative, absolute, file, dir) to workdir paths
/// - only include existing files
pub fn integrate_pathspec(paths: &Vec<PathBuf>) -> HashSet<PathBuf> {
    let mut workdir_paths = HashSet::new();
    for path in paths {
        if path.is_dir() {
            let files = list_files(path).unwrap(); // to workdir
            workdir_paths.extend(files);
        } else {
            workdir_paths.insert(path.to_workdir());
        }
    }
    workdir_paths
}

/// write content to file
/// - create parent directory if not exist
pub fn write_file(content: &[u8], file: &PathBuf) -> io::Result<()> {
    let mut parent = file.clone();
    parent.pop();
    fs::create_dir_all(parent)?;
    let mut file = fs::File::create(file)?;
    file.write_all(content)
}

/// Removing the empty directories in cascade until meet the root of workdir or the current dir
pub fn clear_empty_dir(dir: &Path) {
    let mut dir = if dir.is_dir() {
        dir.to_path_buf()
    } else {
        dir.parent().unwrap().to_path_buf()
    };

    let repo = storage_path();
    // CAN NOT remove .libra & current dir
    while !is_sub_path(&repo, &dir) && !is_cur_dir(&dir) {
        if is_empty_dir(&dir) {
            fs::remove_dir(&dir).unwrap();
        } else {
            break; // once meet a non-empty dir, stop
        }
        dir.pop();
    }
}

pub fn is_empty_dir(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    fs::read_dir(dir).unwrap().next().is_none()
}

pub fn is_cur_dir(dir: &Path) -> bool {
    PathAbs::new(dir).unwrap() == PathAbs::new(cur_dir()).unwrap()
}

/// transform path to string, use '/' as separator even on windows
/// TODO test on windows
/// TODO maybe 'into_os_string().into_string().unwrap()' is good
pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// extend hash, panic if not valid or ambiguous
pub fn get_commit_base(commit_base: &str) -> Result<SHA1, String> {
    let storage = objects_storage();

    let commits = storage.search(commit_base);
    if commits.is_empty() {
        return Err(format!("fatal: invalid reference: {}", commit_base));
    } else if commits.len() > 1 {
        return Err(format!("fatal: ambiguous argument: {}", commit_base));
    }
    if !storage.is_object_type(&commits[0], ObjectType::Commit) {
        Err(format!(
            "fatal: reference is not a commit: {}, is {}",
            commit_base,
            storage.get_object_type(&commits[0]).unwrap()
        ))
    } else {
        Ok(commits[0])
    }
}

/// Get the repository name from the url
/// - e.g. https://github.com/web3infra-foundation/mega.git/ -> mega
/// - e.g. https://github.com/web3infra-foundation/mega.git -> mega
pub fn get_repo_name_from_url(mut url: &str) -> Option<&str> {
    if url.ends_with('/') {
        url = &url[..url.len() - 1];
    }
    let repo_start = url.rfind('/')? + 1;
    let repo_end = url.rfind('.')?;
    Some(&url[repo_start..repo_end])
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::utils::test;
    use std::env;
    use std::path::PathBuf;

    #[test]
    fn cur_dir_returns_current_directory() {
        let expected = env::current_dir().unwrap();
        let actual = cur_dir();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_is_sub_path() {
        assert!(is_sub_path("src/main.rs", "src"));
        assert!(is_sub_path("src/main.rs", "src/"));
        assert!(is_sub_path("src/main.rs", "src/main.rs"));
    }

    #[tokio::test]
    async fn test_to_workdir_path() {
        test::setup_with_new_libra().await;
        let workdir_path = to_workdir_path("src/main.rs");
        assert_eq!(workdir_path, PathBuf::from("src/main.rs"));
    }
}
