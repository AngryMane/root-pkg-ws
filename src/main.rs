use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::io::{self, ErrorKind};
use std::str::FromStr;

use cargo_metadata::CargoOpt;


#[cfg(feature = "cargo-util-schemas")]
use cargo_util_schemas::core::PackageIdSpec;

use clap::Parser;
use git2::{Cred, Oid, RemoteCallbacks};
use glob::glob;
use indexmap::IndexSet;
use tempfile::tempdir;

#[derive(Parser)]
#[command(name = "root-pkg-ws")]
#[command(author = "Joel Winarske <joel.winarske@gmail.com>")]
#[command(version = "1.0")]
#[command(about = "Lists Yocto Recipe for a Root Package Workspace", long_about = None)]
struct Cli {
    #[arg(long)]
    manifest_path: String,
}

#[derive(Eq, Hash, PartialEq)]
struct GitRepo {
    url: String,
    tag: Option<String>,
    branch: Option<String>,
    commit: Option<String>,
}

#[cfg(feature = "cargo-util-schemas")]
fn register_git_package(git: &mut IndexSet<GitRepo>, package: &PackageIdSpec, git_reference: &cargo_util_schemas::core::GitReference) -> Result<(), Box<dyn std::error::Error>> {
    let name = package.name().to_owned();
    let url = package.url().ok_or(format!("{}  doesn't have url.", name))?;
    match git_reference {
        cargo_util_schemas::core::GitReference::Tag(tag) => { 
            println!("    {}/{}", url, tag); 
            git.insert(GitRepo {url: url.to_string(), tag: Some(tag.clone()), branch: None, commit: None });
        },
        cargo_util_schemas::core::GitReference::Branch(branch) => {
            println!("    {}/{}", url, branch); 
            git.insert(GitRepo {url: url.to_string(), tag: None, branch: Some(branch.clone()), commit: None });
        },
        cargo_util_schemas::core::GitReference::Rev(reference) => { 
            println!("    {}/{}", url, reference); 
            git.insert(GitRepo {url: url.to_string(), tag: None, branch: None, commit: Some(reference.clone()) });
        },
        cargo_util_schemas::core::GitReference::DefaultBranch => {},
    }

    Ok(())

    //let repo: Vec<_> = iter[2].split('+').collect();
    //let repository = repo[1].replace(")", "");
    //let elements: Vec<_> = repository.split(&['?', '#'][..]).collect();
    //let url = elements[0].to_owned();
    //let commit;
    //if elements.len() > 2 {
    //    commit = elements[2].to_owned();
    //} else {
    //    commit = elements[1].to_owned();
    //}
    //let git_repo = GitRepo {
    //    url,
    //    commit,
    //};

    //todo!()
}

#[cfg(feature = "cargo-util-schemas")]
fn register_path_package(files: &mut Vec<String>, package: PackageIdSpec) -> Result<(), Box<dyn std::error::Error>> {
    let url = package.url().unwrap();
    println!("    {}", url.to_string());
    Ok(())
}

#[cfg(feature = "cargo-util-schemas")]
fn register_registry_package(crates: &mut IndexSet<String>, package: PackageIdSpec) -> Result<(), Box<dyn std::error::Error>> {
    let name = package.name().to_string();
    let url = package.url().ok_or(format!("{}  doesn't have url.", name))?;
    let url_str = if url.to_string() == "https://github.com/rust-lang/crates.io-index" { "crate://crates.io".to_owned() } else { "crate://".to_owned() + url.authority() + url.path() };
    // ignore query and gragment because crate.py in meta-rust adds '/download' path. 
    // See https://github.com/meta-rust/meta-rust/blob/a5136be2ba408af1cc8afcde1c8e3d787dadd934/lib/crate.py#L82
    let version = package.version().ok_or(format!("{}  doesn't have version.", name))?;
    let crate_repo = format!("{}/{}/{}", url_str, name, version.to_string());
    println!("    {}", crate_repo);
    crates.insert(crate_repo);
    Ok(())
}

#[cfg(feature = "cargo-util-schemas")]
fn register_package(crates: &mut IndexSet<String>, git: &mut IndexSet<GitRepo>, files: &mut Vec<String>, spec: &String) -> Result<(), Box<dyn std::error::Error>> {
    let package = PackageIdSpec::parse(spec)?;
    match package.kind().ok_or("This package doesn't have any KIND.")? {
        cargo_util_schemas::core::SourceKind::Git(git_reference) => register_git_package(git, &package, git_reference)?, 
        cargo_util_schemas::core::SourceKind::Path => register_path_package(files, package)?, 
        cargo_util_schemas::core::SourceKind::Registry => register_registry_package(crates, package)?, 
        cargo_util_schemas::core::SourceKind::SparseRegistry => register_registry_package(crates, package)?,
        _ => println!("[not handled] {}", spec), // PackageIdSpec::parse does not return Directory or LocalRegistry
    }

    Ok(())
}

#[cfg(not(feature = "cargo-util-schemas"))]
fn register_package(crates: &mut IndexSet<String>, git: &mut IndexSet<GitRepo>, file_list: &mut Vec<String>, spec: &String) -> Result<(), Box<dyn std::error::Error>> {
    let iter: Vec<_> = spec.split_whitespace().collect();
    if iter[2] == "(registry+https://github.com/rust-lang/crates.io-index)" {
        let mut crate_repo: String = "crate://crates.io/".to_owned();
        let crate_name: String = iter[0].to_owned();
        let crate_version: String = iter[1].to_owned();

        crate_repo.push_str(&crate_name);
        crate_repo.push_str(&*"/".to_owned());
        crate_repo.push_str(&crate_version);

        crates.insert(crate_repo);
    } else if iter[2].contains("(path+") {
        let repo: Vec<_> = iter[2].split('+').collect();
        let repository = repo[1].replace(")", "");
        let path: Vec<_> = repository.split("file://").collect();
        file_list.push(path[1].to_owned());
    } else if iter[2].contains("(git+") {
        // repo = ["(git", "https://github.com/Stebalien/tempfile.git?branch=master#e5418bd64758d1d0444e9158005f00ca7d2bc6ee)"]
        let repo: Vec<_> = iter[2].split('+').collect();
        // repository = "https://github.com/Stebalien/tempfile.git?branch=master#e5418bd64758d1d0444e9158005f00ca7d2bc6ee"
        let repository = repo[1].replace(")", "");
        // elements = ["https://github.com/Stebalien/tempfile.git", "branch=master", "e5418bd64758d1d0444e9158005f00ca7d2bc6ee"]
        let elements: Vec<_> = repository.split(&['?', '#'][..]).collect();

        let url = elements[0].to_owned();
        let commit;
        if elements.len() > 2 {
            commit = elements[2].to_owned();
        } else {
            commit = elements[1].to_owned();
        }
        let git_repo = GitRepo {
            url,
            tag: None,
            branch: None,
            commit: Some(commit),
        };
        git.insert(git_repo);
    } else {
        println!("[not handled] {}", iter[2]);
    }

    Ok(())
}

fn dump_metadata(path: impl Into<PathBuf>, crates: &mut IndexSet<String>, git: &mut IndexSet<GitRepo>) -> Vec<String> {
    let mut file_list = Vec::new();

    let _metadata: cargo_metadata::Metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(path)
        .features(CargoOpt::AllFeatures)
        .exec()
        .unwrap();

    //println!("workspace_root: {}", _metadata.workspace_root);
    //println!("target_directory: {}", _metadata.target_directory);

    //let _members = _metadata.workspace_members;
    //for _member in _members.iter() {
    // println!("member: {}", _member.repr);
    //}

    let _resolve = _metadata.resolve.unwrap();
    let _nodes: Vec<cargo_metadata::Node> = _resolve.nodes;
    for _node in _nodes.iter() {
        if let Ok(_) = register_package(crates, git, &mut file_list, &_node.id.repr) {
            continue
        }

        let iter: Vec<_> = _node.id.repr.split_whitespace().collect();
        if iter[2] == "(registry+https://github.com/rust-lang/crates.io-index)" {
            let mut crate_repo: String = "crate://crates.io/".to_owned();
            let crate_name: String = iter[0].to_owned();
            let crate_version: String = iter[1].to_owned();

            crate_repo.push_str(&crate_name);
            crate_repo.push_str(&*"/".to_owned());
            crate_repo.push_str(&crate_version);

            crates.insert(crate_repo);
        } else if iter[2].contains("(path+") {
            let repo: Vec<_> = iter[2].split('+').collect();
            let repository = repo[1].replace(")", "");
            let path: Vec<_> = repository.split("file://").collect();
            file_list.push(path[1].to_owned());
        } else if iter[2].contains("(git+") {
            let repo: Vec<_> = iter[2].split('+').collect();
            let repository = repo[1].replace(")", "");
            let elements: Vec<_> = repository.split(&['?', '#'][..]).collect();
            let url = elements[0].to_owned();
            let commit;
            if elements.len() > 2 {
                commit = elements[2].to_owned();
            } else {
                commit = elements[1].to_owned();
            }
            let git_repo = GitRepo {
                url,
                tag: None,
                branch:  None,
                commit: Some(commit),
            };
            git.insert(git_repo);
        } else {
            println!("[not handled] {}", iter[2]);
        }
    }

    return file_list;
}

fn get_repo_folder_name(url: String) -> String {
    let last = url.split('/')
        .last()
        .unwrap()
        .to_string();
    let res: Vec<_> = last.split(".git").collect();
    return res[0].to_string();
}

fn main() {
    let cli = Cli::parse();
    //println!("manifest-path: {:?}", cli.manifest_path);

    let mut crate_list = IndexSet::new();
    let mut git_list = IndexSet::new();

    let _ = dump_metadata(cli.manifest_path, &mut crate_list, &mut git_list);

    //println!();
    //for _file in file_list {
        //let _ = dump_metadata(format!("{}/Cargo.toml", _file), &mut crate_list, &mut git_list);
        //println!("{}", _file);
    //}

    println!();
    println!("SRC_URI += \" \\");
    //let mut count = 0;
    for _crate in crate_list.iter() {
        println!("    {} \\", _crate);
        //count = count + 1;
    }
    //println!("\nCount: {}", count);

    let dir = tempdir().unwrap();

    // Prepare callbacks.
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            Path::new(&format!("{}/.ssh/id_rsa", env::var("HOME").unwrap())),
            None,
        )
    });

    // Prepare fetch options.
    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(callbacks);

    // Prepare builder.
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fo);

    for _git in git_list.iter() {
        let protocol: Vec<_> = _git.url.split("://").collect();
        let folder = get_repo_folder_name(protocol[1].to_string());
        if let Some(branch) = _git.branch.clone() {
            println!("    git://{};lfs=0;nobranch=1;branch={};protocol={};destsuffix={};name={} \\", protocol[1], branch, protocol[0], folder, folder);
        } else if let Some(tag) = _git.tag.clone() {
            println!("    git://{};lfs=0;nobranch=1;tag={};protocol={};destsuffix={};name={} \\", protocol[1], tag, protocol[0], folder, folder);
        } else {
            println!("    git://{};lfs=0;nobranch=1;protocol={};destsuffix={};name={} \\", protocol[1], protocol[0], folder, folder);
        }

        // The following appears to be unnecessary processing.
        ////let a = PathBuf::from_str(".").unwrap();
        ////let folder = a.join(sub_folder);
        //let sub_folder = get_repo_folder_name(_git.url.to_string());
        //let folder = dir.path().join(sub_folder);

        //let repo = builder.clone(&_git.url, Path::new(&folder)).expect("failed to clone repository");

        //let oid = Oid::from_str(&_git.commit).unwrap();
        //let commit = repo.find_commit(oid).unwrap();

        //let _ = repo.branch(
        //    &_git.commit,
        //    &commit,
        //    false,
        //);

        //let obj = repo.revparse_single(&("refs/heads/".to_owned() + &_git.commit)).unwrap();

        //let _ = repo.checkout_tree(
        //    &obj,
        //    None,
        //);

        //let _ = repo.set_head(&("refs/heads/".to_owned() + &_git.commit));

        //let _glob = String::from(folder.join("**/Cargo.toml").to_string_lossy());
        //for entry in glob(&_glob).unwrap() {
        //    match entry {
        //        Ok(manifest) => {
        //            let mut _git_list = IndexSet::new();
        //            let _ = dump_metadata(manifest, &mut crate_list, &mut _git_list);
        //        }
        //        Err(e) => println!("Err: {:?}", e),
        //    }
        //}
    }
    dir.close().unwrap();
    println!("\"\n");

    for _git in git_list.iter().filter(|&x| x.commit != None) {
        let protocol: Vec<_> = _git.url.split("://").collect();
        let folder = get_repo_folder_name(protocol[1].to_string());
        println!("SRCREV_FORMAT .= \"_{}\"", folder);
        println!("SRCREV_{} = \"{}\"", folder, _git.commit.as_ref().unwrap());
    }

    if !git_list.is_empty() {
        println!();
        println!("EXTRA_OECARGO_PATHS += \"\\");
        for _git in git_list.iter() {
            let protocol: Vec<_> = _git.url.split("://").collect();
            let folder = get_repo_folder_name(protocol[1].to_string());
            println!("    ${{WORKDIR}}/{} \\", folder);
        }
        println!("\"");
    }
}
