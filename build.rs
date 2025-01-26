use std::path::PathBuf;
use std::{env, fs};

use walkdir::WalkDir;

static PICO_PREFIX: &str = "vendor/pico/scss";


#[cfg(target_os = "windows")]
use winres::WindowsResource;

fn main() {
    let theme_colors = vec![
        "amber", "blue", "cyan", "fuchsia", "green", "grey", "indigo", "jade", "lime", "orange",
        "pink", "pumpkin", "purple", "red", "sand", "slate", "violet", "yellow", "zinc",
    ];

    // Get the output directory from cargo
    let out_dir =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR environment variable not set by cargo"));
    let pico_dir = out_dir.join("pico");
    let theme_dir = pico_dir.join("theme");
    let scss_dir = pico_dir.join("scss");

    // let archive = zip::ZipWriter::new(std::fs::File::create("pico.zip").expect("Failed to create zip file"));

    // Create temp directory if it doesn't exist
    fs::create_dir_all(&theme_dir).expect("Failed to create theme directory");
    fs::create_dir_all(&scss_dir).expect("Failed to create pico directory");

    // Define all versions to generate
    let versions = vec![
        (
            "pico",
            r#"@use "../scss" with (
        $theme-color: "{color}"
      );"#,
        ),
        (
            "pico.classless",
            r#"@use "../scss" with (
        $theme-color: "{color}",
        $enable-semantic-container: true,
        $enable-classes: false
      );"#,
        ),
        (
            "pico.fluid.classless",
            r#"@use "../scss" with (
        $theme-color: "{color}",
        $enable-semantic-container: true,
        $enable-viewport: false,
        $enable-classes: false
      );"#,
        ),
        (
            "pico.conditional",
            r#"@use "../scss" with (
        $theme-color: "{color}",
        $parent-selector: ".pico"
      );"#,
        ),
        (
            "pico.classless.conditional",
            r#"@use "../scss" with (
        $theme-color: "{color}",
        $enable-semantic-container: true,
        $enable-classes: false,
        $parent-selector: ".pico"
      );"#,
        ),
        (
            "pico.fluid.classless.conditional",
            r#"@use "../scss" with (
        $theme-color: "{color}",
        $enable-semantic-container: true,
        $enable-viewport: false,
        $enable-classes: false,
        $parent-selector: ".pico"
      );"#,
        ),
    ];

    // Generate files for each theme color and version
    for color in theme_colors {
        for (version_name, template) in &versions {
            let content = template.replace("{color}", color);
            let filename = format!("{}.{}.scss", version_name, color);
            let file_path = theme_dir.join(&filename);
            fs::write(file_path, content).expect("Failed to write file");
        }
    }

    // now walkdir the third-party/pico/scss directory and copy everything to the output directory
    for entry in WalkDir::new(PICO_PREFIX) {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        if path.is_file() {
            let relative = path
                .strip_prefix(PICO_PREFIX)
                .expect("Failed to strip prefix");
            let dest = scss_dir.join(relative);
            let parent = dest.parent().expect("Failed to get parent");
            fs::create_dir_all(parent).expect("Failed to create destination parent directory");
            fs::copy(path, dest).expect("Failed to copy file");
        }
    }

    // Tell cargo to rerun this script if the build script changes
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rerun-if-changed={PICO_PREFIX}");

    #[cfg(target_os = "windows")]
    WindowsResource::new()
        .set_icon("wix/lilgux.ico")
        .compile()?;
}
