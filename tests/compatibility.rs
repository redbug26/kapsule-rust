use kapsule::{RunOptions, check, run};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}
fn options() -> RunOptions {
    RunOptions {
        headless_frames: Some(5),
        ..Default::default()
    }
}
fn app(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("apps.toml"),
        "[application]\nid='probe'\n[runtime]\nmain='app.lua'\n",
    )
    .unwrap();
    fs::write(dir.path().join("app.lua"), script).unwrap();
    dir
}
#[test]
fn store_kapsules_load_and_draw_without_modification() {
    for name in [
        "ascii",
        "calculator",
        "calendar",
        "snake",
        "tetris",
        "rockland",
        "git_repo",
        "template_lua_app",
    ] {
        check(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let sandbox = tempfile::tempdir().unwrap();
        copy_tree(&fixture(name), sandbox.path());
        run(sandbox.path(), &[], None, &options()).unwrap_or_else(|e| panic!("{name}: {e:#}"));
    }
}
#[test]
fn modules_lifecycle_args_cwd_and_sibling_imports() {
    let app = app(r#"
local kkc = require('kkc')
local gfx = require('kkc-graphics')
local fs = require('kkc-fs')
local rand = require('kkc-rand')
local dialog = require('kkc-dialog')
assert(require('sibling') == 42)
assert(require('kkc-key').HAS_RELEASE_EVENTS == false)
assert(require('kkc-mouse').LEFT == 'left')
assert(require('kkc-audio').is_audio_playing() == false)
assert(kkc.args()[1] == '--literal')
assert(kkc.get_cwd() == kkc.cwd)
assert(require('kkc-shell').cwd() == kkc.cwd)
fs.write_text('nested/test.txt', 'hello')
assert(fs.exists('nested/test.txt') and fs.read_text('nested/test.txt') == 'hello')
assert(fs.is_dir('nested'))
local found = false
for _, name in ipairs(fs.list_dir('nested')) do if name == 'test.txt' then found = true end end
assert(found)
rand.seed(42); local a = rand.int(1, 6); rand.seed(42); assert(rand.int(6, 1) == a)
assert(rand.float() >= 0 and rand.float() < 1)
local spec = dialog.confirm_box({ buttons={ items={{name='yes'}} }, callback=function() return {id='accept'} end })
assert(spec.buttons.items[1].callback == 'accept' and type(spec.callback) == 'table')
local result = dialog.select_with_checks('empty', {}, 1, {{ label='Saved', checked=true }})
assert(result.index == nil and result.checks[1])
local initialized, resized, frames = false, false, 0
return {
init=function(ctx) assert(ctx.id=='probe' and ctx.args[1]=='--literal'); initialized=true end,
resize=function(w,h) assert(initialized and w==78 and h==22); resized=true end,
update=function(dt) assert(resized and dt>0); frames=frames+1; if frames==2 then kkc.quit() end end,
draw=function() gfx.clear(); gfx.color(0x112233,0x445566); gfx.put(1,1,'X'); gfx.box(2,2,3,3); gfx.text(1,3,'abc') end,
shortcuts=function() return {'Esc:Quit'} end
}
"#);
    fs::write(app.path().join("sibling.lua"), "return 42").unwrap();
    run(
        app.path(),
        &["--literal".into()],
        Some(app.path()),
        &options(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(app.path().join("nested/test.txt")).unwrap(),
        "hello"
    );
}
#[test]
fn invalid_manifests_missing_scripts_and_syntax_fail() {
    let dir = app("return {}");
    fs::write(dir.path().join("apps.toml"), "[application]\nid=''\n").unwrap();
    assert!(check(dir.path()).is_err());
    fs::write(
        dir.path().join("apps.toml"),
        "[application]\nid='bad'\n[runtime]\nmain='../outside.lua'\n",
    )
    .unwrap();
    assert!(check(dir.path()).is_err());
    fs::write(
        dir.path().join("apps.toml"),
        "[application]\nid='bad'\n[runtime]\nmain='missing.lua'\n",
    )
    .unwrap();
    assert!(check(dir.path()).is_err());
    let invalid = app("return { invalid lua");
    assert!(check(invalid.path()).is_err());
    let throwing = app("return { draw=function() error('expected failure') end }");
    assert!(
        format!(
            "{:#}",
            run(throwing.path(), &[], None, &options()).unwrap_err()
        )
        .contains("expected failure")
    );
}
#[test]
fn cli_screen_dump_arguments_and_failure_exit_codes() {
    let app = app(
        "return {draw=function() require('kkc-graphics').text(1,1,require('kkc').args()[1]) end}",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_kapsule"))
        .args(["--headless", "1", "--dump-screen"])
        .arg(app.path().join("apps.toml"))
        .args(["--", "--hello"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("--hello"));
    let output = Command::new(env!("CARGO_BIN_EXE_kapsule"))
        .args(["--headless", "0"])
        .arg(app.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
}
#[test]
fn check_does_not_execute_entry_script() {
    let app = app("error('must not execute')");
    check(app.path()).unwrap();
}

#[test]
fn headless_dialog_failure_is_explicit() {
    let app = app("return {init=function() require('kkc-dialog').message('blocked') end}");
    let error = run(app.path(), &[], None, &options()).unwrap_err();
    assert!(format!("{error:#}").contains("headless"));
}
