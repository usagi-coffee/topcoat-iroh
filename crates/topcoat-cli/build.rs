use std::{env, fmt::Write, fs, path::Path};

use toml::{Table, Value};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let browser = root.join("crates/topcoat-runtime/iroh-browser");
    let workspace: Value = toml::from_str(&read(&root.join("Cargo.toml"))).unwrap();
    let mut manifest: Value = toml::from_str(&read(&browser.join("Cargo.toml"))).unwrap();
    for (name, value) in manifest["package"].as_table_mut().unwrap() {
        if value.get("workspace").and_then(Value::as_bool) == Some(true) {
            *value = workspace["workspace"]["package"][name].clone();
        }
    }
    let dependencies = manifest["target"]["cfg(target_family = \"wasm\")"]["dependencies"]
        .as_table_mut()
        .unwrap();
    for (name, value) in dependencies {
        let mut expanded = match &workspace["workspace"]["dependencies"][name] {
            Value::String(version) => {
                Table::from_iter([("version".into(), Value::String(version.clone()))])
            }
            Value::Table(table) => table.clone(),
            _ => panic!("invalid workspace dependency {name}"),
        };
        expanded.extend(
            value
                .as_table()
                .unwrap()
                .iter()
                .filter(|(key, _)| *key != "workspace")
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        // The bindings must match the generator linked into this CLI.
        if name == "wasm-bindgen" {
            expanded.insert(
                "version".into(),
                workspace["workspace"]["dependencies"]["wasm-bindgen-cli-support"].clone(),
            );
        }
        *value = Value::Table(expanded);
    }
    manifest.as_table_mut().unwrap().remove("lints");
    manifest
        .as_table_mut()
        .unwrap()
        .insert("workspace".into(), Value::Table(Table::new()));
    let mut generated = format!(
        "pub const MANIFEST: &str = {:?};\n",
        toml::to_string(&manifest).unwrap()
    );
    writeln!(
        generated,
        "pub const LOCK: &str = {:?};",
        read(&root.join("Cargo.lock"))
    )
    .unwrap();
    for (constant, directory) in [("SOURCES", "src"), ("LAUNCHER", "launcher")] {
        writeln!(generated, "pub const {constant}: &[(&str, &[u8])] = &[").unwrap();
        println!(
            "cargo:rerun-if-changed={}",
            browser.join(directory).display()
        );
        let mut files = fs::read_dir(browser.join(directory))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        files.sort();
        for path in files {
            if !path.is_file() || path.file_name().unwrap().to_string_lossy().contains("test") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path.file_name().unwrap().to_str().unwrap();
            writeln!(
                generated,
                "({name:?}, include_bytes!({:?})),",
                path.canonicalize().unwrap().to_str().unwrap()
            )
            .unwrap();
        }
        generated.push_str("];\n");
    }
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("iroh_bundle.rs"),
        generated,
    )
    .unwrap();
}

fn read(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(path).unwrap()
}
