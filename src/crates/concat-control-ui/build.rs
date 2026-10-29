//! Compile the small secondary view without regenerating the main editor UI.
fn write_if_changed(path:&std::path::Path,text:&str) {
    if std::fs::read_to_string(path).ok().as_deref()!=Some(text){std::fs::write(path,text).unwrap();}
}
fn main() {
    use std::{env,fs,path::PathBuf};
    let here=PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let shared=here.join("../concat/ui").canonicalize().unwrap();
    let out=PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let source=shared.join("workspace/studio-ai-control.slint");
    println!("cargo:rerun-if-changed={}",source.display());
    println!("cargo:rerun-if-changed=ui/control.slint");
    println!("cargo:rerun-if-changed=ui/campaign.slint");
    // Export the existing composer and monitor in this view's generated copy.
    // The shared source and the expensive main UI build remain unchanged.
    let mut page=fs::read_to_string(&source).unwrap();
    for name in ["ControlComposer","ControlPlanPanel"] {
        let needle=format!("component {name} inherits");
        assert_eq!(page.matches(&needle).count(),1,"Shared control component changed");
        page=page.replace(&needle,&format!("export {needle}"));
    }
    let shared_path=shared.to_string_lossy().replace('\\',"/").trim_start_matches("//?/").to_owned();
    page=page.replace("from \"../",&format!("from \"{shared_path}/"));
    for name in ["studio-chrome.slint","pane.slint","studio-ai.slint","control-chrome.slint"] {
        page=page.replace(&format!("from \"{name}\""),&format!("from \"{shared_path}/workspace/{name}\""));
    }
    write_if_changed(&out.join("control-shared.slint"),&page);
    let root=fs::read_to_string(here.join("ui/control.slint")).unwrap()
        .replace("@campaign-ui@",&here.join("ui/campaign.slint").to_string_lossy().replace('\\',"/"))
        .replace("@control-shared@",&out.join("control-shared.slint").to_string_lossy().replace('\\',"/"))
        .replace("../../concat/ui",&shared_path);
    let root_path=out.join("control-root.slint");
    write_if_changed(&root_path,&root);
    slint_build::compile(root_path).expect("AI controller window must compile");
}
