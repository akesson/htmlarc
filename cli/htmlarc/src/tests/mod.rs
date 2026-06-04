use std::ffi::OsString;

use insta::assert_snapshot;

use crate::{args::Htmlarc, data_manager::Manager, operator::Operator, process};

#[test]
fn cmd_list_plain() {
    assert_snapshot!(run_cmd(&["list", "file"]), @r"
    List:
    Zephyr
    Ephemeral
    Galvanize
    Obfuscate
    Resilient
    ");
    assert_snapshot!(run_cmd(&["list", "file", "--first-n", "4"]), @r"
    List:
    Zephyr
    Ephemeral
    Galvanize
    Obfuscate
    ");
}

#[test]
fn cmd_list_include() {
    assert_snapshot!(run_cmd(&["list", "file", "--include", "testdata/selection.tsv"]), @r"
    List:
    Zephyr
    Ephemeral
    Galvanize
    ");
    assert_snapshot!(run_cmd(&["list", "file", "--include", "css:h1.test"]), @r"
    List:
    Ephemeral
    ");
    assert_snapshot!(run_cmd(&["list", "file", "--include", "words:Obfuscate,Resilient"]), @r"
    List:
    Obfuscate
    Resilient
    ");
}

#[test]
fn cmd_list_exclude_tsv() {
    assert_snapshot!(run_cmd(&["list", "file", "--exclude", "testdata/selection.tsv"]), @r"
    List:
    Obfuscate
    Resilient
    ");
}

// TODO css select needs rewriting
#[ignore = "css needs fixing"]
#[test]
fn cmd_list_exclude_css() {
    assert_snapshot!(run_cmd(&["list", "file", "--exclude", "css:body>h2"]), @"");
}

#[test]
fn cmd_list_exclude_words() {
    assert_snapshot!(run_cmd(&["list", "file", "--exclude", "words:Ephemeral, Zephyr"]), @r"
    List:
    Galvanize
    Obfuscate
    Resilient
    ");
}

#[test]
fn cmd_list_both_include_exclude() {
    assert_snapshot!(
        run_cmd(&[
            "list",
            "file",
            "--include",
            "words:Galvanize,Resilient,Zephyr",
            "--exclude",
            "words:Resilient",
        ]),
        @r"
    List:
    Zephyr
    Galvanize
    "
    );
}

#[test]
fn cmd_navigate() {
    assert_snapshot!(run_cmd(&["list", "file", "--navigate"]), @"Navigate 5 word(s): Zephyr, Ephemeral, Galvanize, Obfuscate, Resilient");
}

#[test]
fn cmd_diff() {
    assert_snapshot!(run_cmd(&["diff", "file", "diff-file"]), @r"
    List:
    Zephyr
    Ephemeral
    Galvanize
    Obfuscate
    ");
    assert_snapshot!(run_cmd(&["diff", "file", "diff-file", "--navigate"]), @"Navigate diff 4 word(s): Zephyr, Ephemeral, Galvanize, Obfuscate");
}

#[test]
fn cmd_to_folder() {
    assert_snapshot!(run_cmd(&["list", "file", "--to-folder", "output"]), @r#"
    Write List:

    Zephyr:
    [

    <body>
    	<h2 id="test">
    zephyr
    	</h2>
    </body>
    ]

    Ephemeral:
    [

    <body>
    	<h1 class="test">
    ephemeral
    	</h1>
    </body>
    ]

    Galvanize:
    [

    <body>
    	<h1 id="test">
    galvanize
    	</h1>
    </body>
    ]

    Obfuscate:
    [

    <body>
    	<h2 class="test" id="hello">
    obfuscate
    	</h2>
    </body>
    ]

    Resilient:
    [

    <body>
    	<h1>
    resilient
    	</h1>
    </body>
    ]
    "#);
}

fn run_cmd(cmd: &[&str]) -> String {
    let cmd = cmd.iter().map(OsString::from).collect();
    let args = Htmlarc::from_vec(cmd).expect("Couldn't parse args");
    let mut operator = Operator::new();
    // Manager and Operator are the in-memory test fixtures (see data_manager/test_data.rs).
    let data_manager = Manager;

    process(&mut operator, data_manager, args).unwrap();

    operator.string().trim().to_string()
}
