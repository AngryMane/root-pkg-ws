use std::env;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::path::PathBuf;

use cargo_metadata::CargoOpt;

#[cfg(feature = "cargo-util-schemas")]
use cargo_util_schemas::core::PackageIdSpec;

use clap::Parser;
use git2::{Cred, RemoteCallbacks};
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
fn register_git_package(
    git: &mut IndexSet<GitRepo>,
    package: &PackageIdSpec,
    git_reference: &cargo_util_schemas::core::GitReference,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = package.name().to_owned();
    let url = package
        .url()
        .ok_or(format!("{}  doesn't have url.", name))?;
    match git_reference {
        cargo_util_schemas::core::GitReference::Tag(tag) => {
            git.insert(GitRepo {
                url: url.to_string(),
                tag: Some(tag.clone()),
                branch: None,
                commit: None,
            });
        }
        cargo_util_schemas::core::GitReference::Branch(branch) => {
            git.insert(GitRepo {
                url: url.to_string(),
                tag: None,
                branch: Some(branch.clone()),
                commit: None,
            });
        }
        cargo_util_schemas::core::GitReference::Rev(reference) => {
            git.insert(GitRepo {
                url: url.to_string(),
                tag: None,
                branch: None,
                commit: Some(reference.clone()),
            });
        }
        cargo_util_schemas::core::GitReference::DefaultBranch => {
            git.insert(GitRepo {
                url: url.to_string(),
                tag: None,
                branch: None,
                commit: None,
            });
        }
    }

    Ok(())
}

#[cfg(feature = "cargo-util-schemas")]
fn register_path_package(
    files: &mut Vec<String>,
    package: PackageIdSpec,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = package.url().unwrap().to_string();
    let path: Vec<_> = url.split("file://").collect();
    files.push(path[1].to_owned());
    Ok(())
}

#[cfg(feature = "cargo-util-schemas")]
fn register_registry_package(
    crates: &mut IndexSet<String>,
    package: PackageIdSpec,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = package.name().to_string();
    let url = package
        .url()
        .ok_or(format!("{}  doesn't have url.", name))?;
    let url_str = if url.to_string() == "https://github.com/rust-lang/crates.io-index" {
        "crate://crates.io".to_owned()
    } else {
        "crate://".to_owned() + url.authority() + url.path()
    };
    // ignore query and gragment because crate.py in meta-rust adds '/download' path.
    // See https://github.com/meta-rust/meta-rust/blob/a5136be2ba408af1cc8afcde1c8e3d787dadd934/lib/crate.py#L82
    let version = package
        .version()
        .ok_or(format!("{}  doesn't have version.", name))?;
    let crate_repo = format!("{}/{}/{}", url_str, name, version.to_string());
    crates.insert(crate_repo);
    Ok(())
}

#[cfg(feature = "cargo-util-schemas")]
fn register_package(
    crates: &mut IndexSet<String>,
    git: &mut IndexSet<GitRepo>,
    files: &mut Vec<String>,
    spec: &String,
) -> Result<(), Box<dyn std::error::Error>> {
    let package = PackageIdSpec::parse(spec)?;
    match package
        .kind()
        .ok_or("This package doesn't have any KIND.")?
    {
        cargo_util_schemas::core::SourceKind::Git(git_reference) => {
            register_git_package(git, &package, git_reference)?
        }
        cargo_util_schemas::core::SourceKind::Path => register_path_package(files, package)?,
        cargo_util_schemas::core::SourceKind::Registry => {
            register_registry_package(crates, package)?
        }
        cargo_util_schemas::core::SourceKind::SparseRegistry => {
            register_registry_package(crates, package)?
        }
        _ => {
            return Err(Box::new(io::Error::new(
                ErrorKind::InvalidData,
                "Invalid data provided",
            )));
        }
    }

    Ok(())
}

#[cfg(not(feature = "cargo-util-schemas"))]
fn register_package(
    crates: &mut IndexSet<String>,
    git: &mut IndexSet<GitRepo>,
    file_list: &mut Vec<String>,
    spec: &String,
) -> Result<(), Box<dyn std::error::Error>> {
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
            branch: None,
            commit: Some(commit),
        };
        git.insert(git_repo);
    } else {
        return Err(Box::new(io::Error::new(
            ErrorKind::InvalidData,
            "Invalid data provided",
        )));
    }

    Ok(())
}

fn dump_metadata(
    path: impl Into<PathBuf>,
    crates: &mut IndexSet<String>,
    git: &mut IndexSet<GitRepo>,
) -> Vec<String> {
    let mut file_list = Vec::new();

    let _metadata: cargo_metadata::Metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(path)
        .features(CargoOpt::AllFeatures)
        .exec()
        .unwrap();

    let _resolve = _metadata.resolve.unwrap();
    let _nodes: Vec<cargo_metadata::Node> = _resolve.nodes;
    for _node in _nodes.iter() {
        if let Err(e) = register_package(crates, git, &mut file_list, &_node.id.repr) {
            let iter: Vec<_> = _node.id.repr.split_whitespace().collect();
            println!("[not handled] {}: {}", iter[2], e);
        }
    }

    return file_list;
}

fn get_repo_folder_name(url: String) -> String {
    let last = url.split('/').last().unwrap().to_string();
    let res: Vec<_> = last.split(".git").collect();
    return res[0].to_string();
}

fn main() {
    let cli = Cli::parse();
    let mut crate_list = IndexSet::new();
    let mut git_list = IndexSet::new();
    let _ = dump_metadata(cli.manifest_path, &mut crate_list, &mut git_list);

    println!();
    println!("SRC_URI += \" \\");
    for _crate in crate_list.iter() {
        println!("    {} \\", _crate);
    }

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
            println!(
                "    git://{};lfs=0;nobranch=1;branch={};protocol={};destsuffix={};name={} \\",
                protocol[1], branch, protocol[0], folder, folder
            );
        } else if let Some(tag) = _git.tag.clone() {
            println!(
                "    git://{};lfs=0;nobranch=1;tag={};protocol={};destsuffix={};name={} \\",
                protocol[1], tag, protocol[0], folder, folder
            );
        } else {
            println!(
                "    git://{};lfs=0;nobranch=1;protocol={};destsuffix={};name={} \\",
                protocol[1], protocol[0], folder, folder
            );
        }
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
