fn main() {
    use pkg_config::probe_library;
    use std::env;
    use std::path::PathBuf;

    probe_library("readline").unwrap();

    let bindings = bindgen::Builder::default()
        .header_contents(
            "wrapper.h",
            r#"
#include <stdio.h>
#include <readline/readline.h>
"#,
        )
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .prepend_enum_name(false)
        .size_t_is_usize(true)
        .allowlist_function("readline")
        .allowlist_function("rl_.*")
        .allowlist_type("rl_.*")
        .allowlist_var("RL_.*")
        .blocklist_type("FILE") // use FILE from libc
        .blocklist_type("_IO_.*")
        .generate()
        .expect("Unable to generate bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("readline_sys.rs"))
        .expect("Couldn't write bindings!");
}
