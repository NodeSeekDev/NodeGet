use vergen_git2::{Build, Cargo, Emitter, Git2, Rustc};

fn main() {
    let build = Build::builder().build_timestamp(true).build();
    let cargo = Cargo::builder().target_triple(true).build();
    let git2 = Git2::builder()
        .branch(true)
        .sha(true)
        .commit_message(true)
        .commit_timestamp(true)
        .build();
    let rustc = Rustc::builder()
        .channel(true)
        .semver(true)
        .commit_date(true)
        .commit_hash(true)
        .llvm_version(true)
        .build();

    Emitter::default()
        .add_instructions(&build)
        .unwrap()
        .add_instructions(&cargo)
        .unwrap()
        .add_instructions(&git2)
        .unwrap()
        .add_instructions(&rustc)
        .unwrap()
        .emit()
        .unwrap();
}
