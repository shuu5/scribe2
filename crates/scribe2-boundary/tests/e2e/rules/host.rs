// flip-check: moved s2-07l.682
//! host の面と host の群の族の歯（接頭辞 `rules_host_` / `host_group_`・設計 docs/design/carry-prep.md §9 行 j）。
//!
//! 共有の helper と const（`HOST_GOOD` / `host_state_dir` / `rules_dispatch` など）と外形 snapshot の歯は
//! 親 module（`tests/e2e/rules.rs`）に在り、`use super::*` で使う。
//! 歯の本文は親から**挙動不変で移した**もの（`s2-07l.682`）。

use super::*;

/// (a) host の面の 3 種の表を `rules validate --state-dir` が数え `host=present`・宣言順の値と行番号が
/// `Manifest` の口から読める。base は `--state-dir` を受けず従来の 1 行（RED）。
#[test]
fn rules_host_validate_counts_the_three_tables_and_names_the_host_present() {
    let dir = host_state_dir(Some(HOST_GOOD)).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=2 plugins=2 launch-args=2 host=present", embedded_validate_line())],
        "rows / kinds は tracked の面のまま・宣言の数を足す"
    );
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let accounts: Vec<(&str, u64)> = manifest.accounts().iter().map(|found| (found.label(), found.line())).collect();
    let plugins: Vec<(&str, u64)> = manifest.plugins().iter().map(|found| (found.dir(), found.line())).collect();
    let args: Vec<(&str, u64)> = manifest.launch_args().iter().map(|found| (found.value(), found.line())).collect();
    assert_eq!(accounts, [("h1", 3), ("h2", 6)], "口座は宣言順・行番号は host の面の行");
    assert_eq!(plugins, [("plugins/one", 9), ("plugins/two", 12)], "plugin dir は宣言順");
    assert_eq!(args, [("--permission-mode", 15), ("acceptEdits", 18)], "起動引数は宣言順");
    assert_eq!(manifest.rows(), Manifest::embedded().expect("埋め込み").rows(), "rules 行は tracked の面だけ");
    std::fs::remove_dir_all(&dir).ok();
}

/// (b) host の面が無い周は `host=absent`・0 宣言（縮退・rc 0）。`--state-dir` の無い周は従来の 1 行のまま、
/// PATH の無い `--state-dir` は断る（tracked の面だけで黙って通さない）。
#[test]
fn rules_host_validate_without_the_host_file_is_absent_with_zero_declarations() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=0 plugins=0 launch-args=0 host=absent", embedded_validate_line())]
    );
    let bare = embedded_validate_line();
    assert!(bare.starts_with("rules: ok rows=") && !bare.contains(" accounts="), "--state-dir 無しは従来の 1 行: {bare}");
    for bad in [&["validate", "--state-dir"][..], &["validate", "--state-dir", ""], &["validate", "--state-dir", "--rules"]] {
        let refused = rules_dispatch(bad);
        assert_eq!(refused.rc, RC_REFUSED, "{bad:?}: {refused:?}");
        assert!(refused.out.is_empty(), "{bad:?}: stdout へは書かない");
        assert_eq!(refused.err, vec!["rules: --state-dir に PATH が無い line=0".to_owned()], "{bad:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// (c) 壊れた host の面は欠陥を**全件**・行番号付き・`host.toml:` の接頭辞で拒む（rc 1・stdout 0 行）: schema 欠落・
/// 未知 key・型違い・`[[rule]]` の混入。面をまたぐ label の重複（`--rules` の tracked 側と host 側に同じ label）も
/// host の面の行番号で拒む。
#[test]
fn rules_host_rejects_every_defect_with_line_numbers_and_the_face_prefix() {
    let dir = host_state_dir(Some(HOST_DEFECTIVE)).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "stdout へは書かない: {outcome:?}");
    let want = [
        ("schema = 1 が無い", 0),
        ("未知の key color", 6),
        ("value は文字列でなければならない", 9),
        ("[[rule]] は host の面に置けない", 11),
    ];
    assert_eq!(outcome.err.len(), want.len(), "全件・同じ欠陥を 2 行にしない: {:?}", outcome.err);
    for ((reason, line), got) in want.iter().zip(&outcome.err) {
        assert!(got.starts_with("rules: host.toml: "), "面を名指す接頭辞: {got}");
        assert!(got.contains(reason) && got.ends_with(&format!(" line={line}")), "{reason} line={line}: {got}");
    }

    let tracked = dir.join("tracked.toml");
    std::fs::write(&tracked, format!("{GOOD}\n[[account]]\nlabel = \"shared\"\n")).expect("tracked の fixture を書ける");
    std::fs::write(dir.join(vessel::rules::HOST_MANIFEST), "schema = 1\n\n[[account]]\nlabel = \"solo\"\n\n[[account]]\nlabel = \"shared\"\n")
        .expect("host の面を書ける");
    let rules = tracked.display().to_string();
    let crossed = rules_dispatch(&["validate", "--rules", &rules, "--state-dir", &state]);
    assert_eq!(crossed.rc, RC_REFUSED, "{crossed:?}");
    assert_eq!(
        crossed.err,
        vec!["rules: host.toml: label shared が面をまたいで重複する（tracked の manifest にも在る） line=6".to_owned()],
        "面をまたぐ重複は host の面の行で 1 件"
    );
    let alone = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(alone.rc, RC_OK, "埋め込みの面に口座は無い＝同じ host の面が単独では通る: {alone:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (h・`s2-07l.303`) `[[vessel]] repo` 1 行は host の面から読める（`validate --state-dir` が rc 0 で数え、`Manifest::vessel`
/// が dir と見出し行を運ぶ・tracked の面は持たない）。2 行目は**行番号付きで重複として拒む**（`host.toml:` の接頭辞・
/// stdout 0 行・1 行目の値は読まない）。`repo` 欠け・空・未知 key は他の表と同じ拒否形。面をまたぐ 2 行（`--rules` の
/// tracked 側と host 側）も host の面の行番号で拒む。base は `[[vessel]]` を未知の section として拒む（RED）。
#[test]
fn rules_host_vessel_row_is_read_and_duplicates_are_refused() {
    let one = "schema = 1\n\n[[account]]\nlabel = \"h1\"\n\n[[vessel]]\nrepo = \"/srv/vessel\"\n";
    let dir = host_state_dir(Some(one)).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(outcome.out, vec![format!("{} accounts=1 plugins=0 launch-args=0 host=present", embedded_validate_line())]);
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let found = manifest.vessel().expect("[[vessel]] が読める");
    assert_eq!((found.repo(), found.line()), ("/srv/vessel", 6), "dir と見出し行");
    assert_eq!(Manifest::embedded().expect("埋め込み").vessel(), None, "tracked の面は持たない");
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    std::fs::write(&host, format!("{one}\n[[vessel]]\nrepo = \"/srv/other\"\n")).expect("host の面を書ける");
    let dup = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(dup.rc, RC_REFUSED, "{dup:?}");
    assert!(dup.out.is_empty(), "stdout へは書かない: {dup:?}");
    assert_eq!(dup.err, vec!["rules: host.toml: [[vessel]] が重複する（最大 1 行） line=9".to_owned()], "2 行目を行番号で名指す");
    for (body, want) in [
        ("schema = 1\n\n[[vessel]]\n", "rules: host.toml: 必須 key repo が無い line=3"),
        ("schema = 1\n\n[[vessel]]\nrepo = \"\"\n", "rules: host.toml: repo が空である line=3"),
        ("schema = 1\n\n[[vessel]]\nrepo = \"/x\"\nbranch = \"main\"\n", "rules: host.toml: 未知の key branch line=5"),
        ("schema = 1\n\n[[vessel]]\nrepo = 3\n", "rules: host.toml: repo は文字列でなければならない（実 One(Int(3))） line=4"),
    ] {
        std::fs::write(&host, body).expect("host の面を書ける");
        let refused = rules_dispatch(&["validate", "--state-dir", &state]);
        assert_eq!(refused.rc, RC_REFUSED, "{body:?}: {refused:?}");
        assert_eq!(refused.err, vec![want.to_owned()], "{body:?}: 他の表と同じ拒否形・1 件");
    }
    let tracked = dir.join("tracked.toml");
    std::fs::write(&tracked, format!("{GOOD}\n[[vessel]]\nrepo = \"/srv/tracked\"\n")).expect("tracked の fixture を書ける");
    std::fs::write(&host, one).expect("host の面を書ける");
    let rules = tracked.display().to_string();
    let crossed = rules_dispatch(&["validate", "--rules", &rules, "--state-dir", &state]);
    assert_eq!(crossed.rc, RC_REFUSED, "{crossed:?}");
    assert_eq!(crossed.err, vec!["rules: host.toml: [[vessel]] が面をまたいで重複する（最大 1 行） line=6".to_owned()], "面をまたぐ 2 行");
    std::fs::remove_dir_all(&dir).ok();
}

/// (1) 1 行の `[[tick]]` は host の面から読める: `validate --state-dir` は rc 0 で宣言の数の 1 行は表の無い面と同じ字面（表を
/// 数えない＝既存の外形は動かない）・`Manifest::tick` が 2 欄（絶対 path）と見出し行を運ぶ・tracked の面（埋め込み）は持たない。
/// base は `[[tick]]` を未知の section として拒む（RED）。
#[test]
fn rules_host_tick_one_row_is_read_with_two_absolute_fields() {
    let dir = host_state_dir(Some(HOST_TICK)).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(outcome.out, vec![format!("{} accounts=1 plugins=0 launch-args=0 host=present", embedded_validate_line())]);
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let tick = manifest.tick().expect("[[tick]] が読める");
    assert_eq!((tick.unit_dir(), tick.binary(), tick.line()), ("/srv/units", "/opt/bin/scribe2", 6), "2 欄と見出し行");
    assert_eq!(Manifest::embedded().expect("埋め込み").tick(), None, "tracked の面は持たない");
    std::fs::remove_dir_all(&dir).ok();
}

/// (2) 2 行目・相対 path・欠けた欄は行番号つきで断る（`host.toml:` の接頭辞・rc 1・stdout 0 行・1 件ずつ）。tracked の面に置いた
/// 表は 1 表 1 件で断る。
#[test]
fn rules_host_tick_refuses_a_second_row_relative_paths_and_missing_fields() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    for (body, want) in [
        (format!("{HOST_TICK}\n[[tick]]\nunit-dir = \"/srv/other\"\nbinary = \"/opt/bin/other\"\n"), vec!["rules: host.toml: [[tick]] が重複する（最大 1 行） line=10"]),
        (HOST_TICK.replace("\"/srv/units\"", "\"units\""), vec!["rules: host.toml: unit-dir が絶対 path でない: \"units\" line=7"]),
        (HOST_TICK.replace("\"/opt/bin/scribe2\"", "\"bin/scribe2\""), vec!["rules: host.toml: binary が絶対 path でない: \"bin/scribe2\" line=8"]),
        (HOST_TICK.replace("binary = \"/opt/bin/scribe2\"\n", ""), vec!["rules: host.toml: 必須 key binary が無い line=6"]),
        (HOST_TICK.replace("unit-dir = \"/srv/units\"\n", ""), vec!["rules: host.toml: 必須 key unit-dir が無い line=6"]),
    ] {
        std::fs::write(&host, &body).expect("host の面を書ける");
        let refused = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
        assert_eq!(refused.rc, RC_REFUSED, "{body:?}: {refused:?}");
        assert!(refused.out.is_empty(), "{body:?}: stdout へは書かない");
        assert_eq!(refused.err, want, "{body:?}");
    }
    let tracked = dir.join("tracked.toml");
    // `GOOD` は 17 行なので、空行を挟んで足した表の見出しは 19 行目。
    std::fs::write(&tracked, format!("{GOOD}\n[[tick]]\nunit-dir = \"/srv/units\"\nbinary = \"/opt/bin/scribe2\"\n")).expect("tracked の fixture を書ける");
    std::fs::remove_file(&host).ok();
    let outcome = rules_dispatch(&["validate", "--rules", &tracked.display().to_string(), "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert_eq!(outcome.err, vec!["rules: [[tick]] は tracked の manifest に置けない（unit の置き場は host の面だけ） line=19".to_owned()]);
    std::fs::remove_dir_all(&dir).ok();
}

/// (3) 表の無い host は今のまま: `Manifest::tick` は `None`・`validate --state-dir` の 1 行は表を足す前と同じ字面（`HOST_GOOD` の
/// 3 種の表の数だけ・外形 snapshot `rules_external_form` は同じ本文を撃つ）。
#[test]
fn rules_host_tick_absent_table_leaves_the_host_face_unchanged() {
    let dir = host_state_dir(Some(HOST_GOOD)).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.out, vec![format!("{} accounts=2 plugins=2 launch-args=2 host=present", embedded_validate_line())]);
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    assert_eq!(manifest.tick(), None, "表の無い面は None");
    std::fs::remove_dir_all(&dir).ok();
    let absent = host_state_dir(None).expect("tmp の state dir を作れる");
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(absent.as_path())))
        .expect("面が無くても続く");
    assert_eq!(manifest.tick(), None, "面の無い host も None");
    std::fs::remove_dir_all(&absent).ok();
}

/// (a) 2 行の `[[device]]` は host の面から読める: `validate --state-dir` は rc 0 で宣言の数の 1 行は表の無い面と同じ字面（表を
/// 数えない）・合わせた `Manifest::devices` が宣言順の 2 行と各欄と見出し行を運ぶ・埋め込みの面は 0 行。base は `[[device]]` を
/// 未知の section として拒む（RED）。
#[test]
fn rules_host_device_two_rows_are_read_in_declaration_order_with_every_field() {
    let dir = host_state_dir(Some(HOST_DEVICE)).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(outcome.out, vec![format!("{} accounts=0 plugins=0 launch-args=0 host=present", embedded_validate_line())]);
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let facts: Vec<String> = manifest.devices().iter().map(device_facts).collect();
    assert_eq!(
        facts,
        [
            "win-1|me@win|C:/Chrome/chrome.exe|windows|None|[]|None|3",
            "mac-2|me@mac|/Applications/Google Chrome.app|macos|Some(\":1\")|[\"GTK_IM_MODULE=fcitx\", \"XMODIFIERS=@im=fcitx\"]|Some(\"/tmp/prof\")|9",
        ],
        "宣言順・任意の欄は無ければ None と空"
    );
    assert!(Manifest::embedded().expect("埋め込み").devices().is_empty(), "埋め込みの面は 0 行");
    std::fs::remove_dir_all(&dir).ok();
}

/// (b) 欠けた必須の欄・未知の key・`os` の 3 語の外・`ime-env` の形の外と KEY の重複・`name` の重複・空白を含む `name` と `ssh`
/// は、行番号つきで 1 件ずつ断る（`host.toml:` の接頭辞・rc 1・stdout 0 行）。
#[test]
fn rules_host_device_refuses_each_defect_once_with_its_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    let second = "\n[[device]]\nname = \"a\"\nssh = \"me@b\"\nchrome = \"/d\"\nos = \"macos\"\n";
    for (body, want) in [
        (HOST_DEVICE_ONE.replace("chrome = \"/c\"\n", ""), "必須 key chrome が無い line=3"),
        (format!("{HOST_DEVICE_ONE}color = \"red\"\n"), "未知の key color line=8"),
        (HOST_DEVICE_ONE.replace("\"linux\"", "\"beos\""), "os \"beos\" が linux / macos / windows のどれでもない line=7"),
        (format!("{HOST_DEVICE_ONE}ime-env = [\"LANG\"]\n"), "ime-env の要素 \"LANG\" が KEY=VALUE の形でない（= が無い） line=8"),
        (format!("{HOST_DEVICE_ONE}ime-env = [\"A=1\", \"A=2\"]\n"), "ime-env の KEY A が重複する line=8"),
        (format!("{HOST_DEVICE_ONE}{second}"), "端末の名 a が重複する line=9"),
        (HOST_DEVICE_ONE.replace("name = \"a\"", "name = \"a b\""), "name が空白を含む: \"a b\" line=4"),
        (HOST_DEVICE_ONE.replace("\"me@a\"", "\"me @a\""), "ssh が空白を含む: \"me @a\" line=5"),
    ] {
        std::fs::write(&host, &body).expect("host の面を書ける");
        let refused = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
        assert_eq!(refused.rc, RC_REFUSED, "{body:?}: {refused:?}");
        assert!(refused.out.is_empty(), "{body:?}: stdout へは書かない");
        assert_eq!(refused.err, vec![format!("rules: host.toml: {want}")], "{body:?}: 1 件");
    }
    std::fs::write(&host, HOST_DEVICE_ONE).expect("host の面を書ける");
    let alone = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(alone.rc, RC_OK, "崩す前の 1 行は通る: {alone:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (c) tracked の面（`--rules` の写し）に置いた表は 1 表 1 件で断る（中身は検査しない・2 表なら 2 件）。
#[test]
fn rules_host_device_table_on_the_tracked_face_is_refused_once_per_table() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let tracked = dir.join("tracked.toml");
    // `GOOD` は 17 行なので、空行を挟んで足した 2 表の見出しは 19 行目と 25 行目。
    let tables = HOST_DEVICE.trim_start_matches("schema = 1\n\n");
    std::fs::write(&tracked, format!("{GOOD}\n{tables}")).expect("tracked の fixture を書ける");
    let outcome = rules_dispatch(&["validate", "--rules", &tracked.display().to_string(), "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert_eq!(
        outcome.err,
        [19, 25].map(|line| format!("rules: [[device]] は tracked の manifest に置けない（端末の値は host の面だけ） line={line}"))
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (f) dir の host の面で `rules validate --state-dir S` は拒否・stderr は `host.toml:` の 1 行（`absent` に潰れない）。
#[test]
fn rules_host_validate_refuses_a_directory_host_manifest() {
    let dir = host_dir_state().expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "host=absent の行を出さない: {outcome:?}");
    assert_eq!(outcome.err.len(), 1, "1 行: {:?}", outcome.err);
    let first = outcome.err.first().map(String::as_str).unwrap_or_default();
    assert!(first.starts_with("rules: host.toml: "), "面の接頭辞: {first}");
    assert!(first.contains(&dir.join(vessel::rules::HOST_MANIFEST).display().to_string()), "path を名指す: {first}");
    assert!(first.ends_with(" line=0"), "行番号: {first}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (g) 同じ fixture で `fleet usage` も typed に止まる（rc 1・stdout 0 byte・event を書かない）。
#[test]
fn rules_host_directory_host_manifest_stops_fleet_usage_without_events() {
    let dir = host_dir_state().expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let host = dir.join(vessel::rules::HOST_MANIFEST).display().to_string();
    let bin = env!("CARGO_BIN_EXE_scribe2");
    let curl = dir.join("no-curl").display().to_string();
    let usage = Command::new(bin)
        .args(["fleet", "usage", "--state-dir", &state, "--curl", &curl])
        .output()
        .expect("binary を起動できる");
    assert_eq!(usage.status.code(), Some(i32::from(RC_REFUSED)), "{usage:?}");
    assert!(usage.stdout.is_empty(), "stdout は 0 byte: {usage:?}");
    let said = String::from_utf8_lossy(&usage.stderr);
    assert!(said.starts_with("fleet usage: manifest を読めない（rules: host.toml: "), "typed の 1 行: {said}");
    assert!(said.contains(&host) && said.ends_with(" line=0）\n") && said.lines().count() == 1, "path と行番号: {said}");
    assert!(!vessel::fleet::store::events_path(&dir).exists(), "event を書かない");
    std::fs::remove_dir_all(&dir).ok();
}

/// (h) `rules` を引数なし・`rules get` を id なしで撃つと usage 1 行で拒否（行は `--rules PATH` と `--state-dir S` を名指す）。
#[test]
fn rules_host_usage_line_names_both_rules_and_state_dir_flags() {
    let want = "usage: rules <validate|get <id>> [--rules PATH] [--state-dir S]";
    assert!(want.contains("--rules PATH") && want.contains("--state-dir S"), "期待値の自己検査");
    for args in [&[][..], &["get"]] {
        let outcome = rules_dispatch(args);
        assert_eq!(outcome.rc, RC_REFUSED, "{args:?}: {outcome:?}");
        assert!(outcome.out.is_empty(), "{args:?}: stdout へは書かない");
        assert_eq!(outcome.err, vec![want.to_owned()], "{args:?}: usage 1 行");
    }
}

/// (g) tracked の manifest は口座の表を持たない（宣言は host の面にだけ・公開面の情報が減る側）・待ち時間の行は在る。
#[test]
fn rules_host_embedded_manifest_declares_no_account_and_keeps_usage_timeout() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    assert_eq!(manifest.accounts().len(), 0, "tracked の面の口座: {:?}", manifest.accounts());
    assert!(manifest.plugins().is_empty() && manifest.launch_args().is_empty(), "host 固有の値も持たない");
    let timeout = manifest.get("fleet.usage_timeout_s").expect("待ち時間の行が在る");
    assert_eq!(timeout.value, RuleValue::Int(30), "user 裁定 2026-09-12T02:01Z の値");
    assert_eq!(timeout.kind, RuleKind::UsageTimeoutS, "kind");
    assert_eq!(timeout.kind.shape(), ValueShape::Int, "値の形は Int（秒）");
    assert!(timeout.enabled, "既定で効く");
    assert_eq!(timeout.ruling, "user 2026-09-12T02:01Z", "裁定 id");
    assert_eq!(timeout.ruled_at, "2026-09-12", "裁定日");
}

/// (1) host の面の `[[account-group]]` を**既存の読み手 1 本**が読む: 3 key が宣言順で取れ、群も宣言順・行番号は
/// host の面の行。`rules validate --state-dir` は rc 0（口座の数は従来どおり数える＝群は口座の表を増やさない）。
/// 便用の除外の集合（`grouped_accounts`）は各群の今の口座（記録なし＝種・§23 形 1）。tracked の面（埋め込み）は群を 1 つも持たない。
/// base は `[[account-group]]` を未知の section として拒む（RED）。
#[test]
fn host_group_table_is_read_from_the_host_face_with_three_keys() {
    let dir = host_state_dir(Some(HOST_GROUPS)).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=3 plugins=0 launch-args=0 host=present", embedded_validate_line())],
        "群は口座 / plugin / 起動引数の数を動かさない"
    );
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let groups: Vec<(&str, Vec<&str>, Vec<&str>, u64)> = manifest
        .groups()
        .iter()
        .map(|group| {
            let anchors: Vec<&str> = group.anchors().iter().map(String::as_str).collect();
            let accounts: Vec<&str> = group.accounts().iter().map(String::as_str).collect();
            (group.name(), anchors, accounts, group.line())
        })
        .collect();
    assert_eq!(
        groups,
        [
            ("Tier1", vec!["/repo/a", "/repo/b"], vec!["g2", "g1"], 12),
            ("Tier2", vec!["/repo/c"], vec!["g3"], 17),
        ],
        "群も置き場も候補も宣言順（候補の順は label の昇順ではない）"
    );
    // 便用の除外は各群の今の口座だけ（記録なし＝種＝候補の先頭・§23 形 1）。置き場は tmp の 1 段下（host の根の記録を
    // 歯どうしで共有しない）。
    let place = dir.join("place");
    std::fs::create_dir_all(&place).expect("置き場を作れる");
    std::fs::write(place.join(vessel::rules::HOST_MANIFEST), HOST_GROUPS).expect("host の面を書ける");
    let grouped: Vec<String> = vessel::rules::grouped_accounts(&place).expect("除外を解ける").into_iter().collect();
    assert_eq!(grouped, ["g2", "g3"], "便用の除外は各群の今の口座（候補の和ではない）");
    assert!(Manifest::embedded().expect("埋め込み").groups().is_empty(), "tracked の面は群を持たない");
    std::fs::remove_dir_all(&dir).ok();
}

/// (2) host の面が無い周は 0 群で続く（縮退・rc 0・`host=absent`）。除外も 0 件＝便用の候補は今までどおり。
#[test]
fn host_group_absent_host_face_declares_zero_groups() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=0 plugins=0 launch-args=0 host=absent", embedded_validate_line())]
    );
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("面が無くても続く");
    assert!(manifest.groups().is_empty(), "0 群: {:?}", manifest.groups());
    assert_eq!(
        vessel::rules::grouped_accounts(dir.as_path()),
        Ok(std::collections::BTreeSet::new()),
        "置き場から直に読む口も 0 件（便の口はこちらを読む）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (3) tracked の面に群の表が在る周は**未知の表**として行番号付きで断る（`[[rule]]` を host の面で断るのと対称）。
/// 行の中身は検査しない（1 表 1 件）＝未知 key も必須 key の欠けも重ねない。
#[test]
fn host_group_table_on_the_tracked_face_is_refused_as_unknown() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let tracked = dir.join("tracked.toml");
    // `GOOD` は 17 行なので、空行を挟んで足した表の見出しは 19 行目。key は 3 つとも書かない（1 表 1 件の確認）。
    std::fs::write(&tracked, format!("{GOOD}\n[[account-group]]\n")).expect("tracked の fixture を書ける");
    let outcome = rules_dispatch(&["validate", "--rules", &tracked.display().to_string(), "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "stdout へは書かない: {outcome:?}");
    assert_eq!(
        outcome.err,
        vec!["rules: [[account-group]] は tracked の manifest に置けない（群の宣言は host の面だけ） line=19".to_owned()],
        "1 表 1 件・行番号付き・host の面の接頭辞は付かない"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (4) 宣言の欠陥 6 種を行番号付きで**全件**断る（`host.toml:` の接頭辞・stdout 0 行）: 同じ名が 2 行・同じ置き場が
/// 2 つの群・宣言に無い候補・置き場の列が空・候補の列が空・未知の key。最後に、面の中の欠陥で止まった周は**合わせの
/// 検査へ進まない**（同じ本文が未知の候補も持つのに、出るのは面の中の 1 件だけ）。
#[test]
fn host_group_defects_are_refused_with_line_numbers_and_the_face_prefix() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let group = |name: &str, anchors: &str, accounts: &str| {
        format!("\n[[account-group]]\nname = \"{name}\"\nanchors = {anchors}\naccounts = {accounts}\n")
    };
    let tier1 = group("Tier1", "[\"/repo/a\"]", "[\"g1\"]");
    for (body, want) in [
        // 2 つ目の群の見出し（17 行目）で名の重複。
        (format!("{GROUP_HEAD}{tier1}{}", group("Tier1", "[\"/repo/b\"]", "[\"g2\"]")), vec![(17, "群の名 Tier1 が重複する")]),
        // 同じ置き場が 2 つの群に在る（2 つ目の群の見出し）。
        (format!("{GROUP_HEAD}{tier1}{}", group("Tier2", "[\"/repo/a\"]", "[\"g2\"]")), vec![(17, "置き場 /repo/a が 2 つの群に在る")]),
        // 宣言に無い候補（合わせの検査・群の見出し行）。
        (format!("{GROUP_HEAD}{}", group("Tier1", "[\"/repo/a\"]", "[\"nope\"]")), vec![(12, "群 Tier1 の候補 nope が宣言された口座に無い")]),
        // 置き場の列が空（14 行目 = anchors の行）。
        (format!("{GROUP_HEAD}{}", group("Tier1", "[]", "[\"g1\"]")), vec![(14, "anchors の 配列が空である")]),
        // 候補の列が空（15 行目 = accounts の行）。
        (format!("{GROUP_HEAD}{}", group("Tier1", "[\"/repo/a\"]", "[]")), vec![(15, "accounts の 配列が空である")]),
        // 未知の key（16 行目）。
        (format!("{GROUP_HEAD}{tier1}model = \"opus\"\n"), vec![(16, "未知の key model")]),
    ] {
        let outcome = group_validate(dir.as_path(), &body);
        assert_eq!(outcome.rc, RC_REFUSED, "{body}: {outcome:?}");
        assert!(outcome.out.is_empty(), "{body}: stdout へは書かない");
        assert_eq!(outcome.err.len(), want.len(), "{body}: 全件・同じ欠陥を 2 行にしない: {:?}", outcome.err);
        for ((line, reason), got) in want.iter().zip(&outcome.err) {
            assert!(got.starts_with("rules: host.toml: "), "{body}: 面を名指す接頭辞: {got}");
            assert!(got.contains(reason) && got.ends_with(&format!(" line={line}")), "{body}: {reason} line={line}: {got}");
        }
    }
    // 面の中の欠陥（名の重複）と合わせの欠陥（未知の候補 nope）を同時に持つ本文は、面の中の 1 件だけを出す。
    let both = format!("{GROUP_HEAD}{tier1}{}", group("Tier1", "[\"/repo/b\"]", "[\"nope\"]"));
    let outcome = group_validate(dir.as_path(), &both);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert_eq!(
        outcome.err,
        vec!["rules: host.toml: 群の名 Tier1 が重複する line=17".to_owned()],
        "面の中で止まった周は合わせの検査へ進まない"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (d) 同じ候補の列 [g1, g2, g3] を宣言した 2 群の記録なしの周は、今の口座が宣言順に先頭 g1 と 2 番目 g2（種は前の群の種で
/// ない最初の候補）で、便用の除外も 2 つ。(f) 後の群に記録（g3）が在れば記録が勝ち、前の群は種 g1 のまま。
/// base は両群とも先頭 g1（除外は 1 つ）→ RED。
#[test]
fn host_group_seed_two_groups_with_the_same_candidates_take_the_first_and_the_second() {
    use vessel::hook::group::Source;
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let place = dir.join("place");
    let same = "[\"g1\", \"g2\", \"g3\"]";
    let body = format!("{GROUP_HEAD}{}{}", seed_group("Tier1", "/repo/a", same), seed_group("Tier2", "/repo/b", same));
    assert_eq!(
        seed_currents(&place, &body),
        [("g1".to_owned(), Source::Seed), ("g2".to_owned(), Source::Seed)],
        "種は宣言順に先頭と 2 番目"
    );
    let grouped: Vec<String> = vessel::rules::grouped_accounts(&place).expect("除外を解ける").into_iter().collect();
    assert_eq!(grouped, ["g1", "g2"], "便用の除外は 2 つ（同じ口座に畳まれない）");
    let record = vessel::hook::group::current_path(&vessel::seat::host_groups_dir(&place), "Tier2");
    std::fs::create_dir_all(record.parent().expect("記録の dir")).expect("群用 dir を作れる");
    std::fs::write(&record, "account=g3\nts=2026-09-25T00:00:00Z\nreason=move\nprevious=g2\n").expect("記録を書ける");
    assert_eq!(
        seed_currents(&place, &body),
        [("g1".to_owned(), Source::Seed), ("g3".to_owned(), Source::Record)],
        "記録 > 種（記録は種に依らない）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (e) 候補 1 つ（g1）を共有する 2 群は、後の群の種を決める候補が無い＝面の欠陥（2 番目の群の見出し 17 行目・`host.toml:` の
/// 接頭辞・全件 1 件）で、便用の除外も面の欠陥で typed に止まる（fail-closed・前の群の種に読み替えない）。
/// base は両群とも種 g1 で rc 0 → RED。
#[test]
fn host_group_seed_one_shared_candidate_is_a_defect_on_the_second_group_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let body = format!("{GROUP_HEAD}{}{}", seed_group("Tier1", "/repo/a", "[\"g1\"]"), seed_group("Tier2", "/repo/b", "[\"g1\"]"));
    let outcome = group_validate(dir.as_path(), &body);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "stdout へは書かない: {outcome:?}");
    assert_eq!(outcome.err, vec!["rules: host.toml: 群 Tier2 の種を決める候補が無い line=17".to_owned()], "2 番目の群の行で 1 件");
    assert!(
        matches!(vessel::rules::grouped_accounts(dir.as_path()), Err(vessel::rules::GroupedError::Manifest(_))),
        "除外は面の欠陥で止まる"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (g) 群 1 つの host は種が候補の先頭のまま（候補の順は label の昇順ではない＝[g2, g1] の種は g2）。
#[test]
fn host_group_seed_single_group_keeps_the_first_candidate() {
    use vessel::hook::group::Source;
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let body = format!("{GROUP_HEAD}{}", seed_group("Tier1", "/repo/a", "[\"g2\", \"g1\"]"));
    assert_eq!(seed_currents(&dir.join("place"), &body), [("g2".to_owned(), Source::Seed)], "先頭のまま");
    std::fs::remove_dir_all(&dir).ok();
}

/// (j) 名が Tier と数字の形でない群（alpha）は群の見出し行（12 行目）の欠陥で 1 件（`host.toml:` の接頭辞）。base は通る（RED）。
#[test]
fn host_group_tier_non_tier_name_is_a_defect_on_the_group_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("alpha", "g1")]);
    assert_eq!(err.len(), 1, "1 件: {err:?}");
    let got = err.first().cloned().unwrap_or_default();
    assert!(got.starts_with("rules: host.toml: 群の名 alpha "), "面と名を名指す: {got}");
    assert!(got.contains("Tier と数字の形でない") && got.ends_with(" line=12"), "群の行の形の欠陥: {got}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (k) Tier2, Tier1 の宣言順は 2 番目の群の見出し行（17 行目）の欠陥で 1 件（数字の昇順でない）。
#[test]
fn host_group_tier_descending_order_is_a_defect_on_the_second_group_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("Tier2", "g1"), ("Tier1", "g2")]);
    assert_eq!(err.len(), 1, "1 件: {err:?}");
    let got = err.first().cloned().unwrap_or_default();
    assert!(got.starts_with("rules: host.toml: 群 Tier1 "), "後の群を名指す: {got}");
    assert!(got.contains("前の群より大きくない") && got.ends_with(" line=17"), "2 番目の群の行: {got}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (l) Tier1, Tier1 は名の重複の 1 件だけ（昇順の欠陥を重ねない＝同じ欠陥を 2 行にしない）。
#[test]
fn host_group_tier_same_name_twice_is_only_the_duplicate_defect() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("Tier1", "g1"), ("Tier1", "g2")]);
    assert_eq!(err, vec!["rules: host.toml: 群の名 Tier1 が重複する line=17".to_owned()], "名の重複の 1 件だけ");
    std::fs::remove_dir_all(&dir).ok();
}

/// (m) 先頭の 0（Tier01・Tier0）・数字なし（Tier）・接頭辞の綴り違い（tier1）・数字の後ろの字（Tier1a）は形の欠陥で 1 件。
#[test]
fn host_group_tier_malformed_digits_are_a_form_defect() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    for name in ["Tier01", "Tier0", "Tier", "tier1", "Tier1a"] {
        let err = tier_refusals(&dir, &[(name, "g1")]);
        assert_eq!(err.len(), 1, "{name}: 1 件: {err:?}");
        let got = err.first().cloned().unwrap_or_default();
        assert!(got.contains(&format!("群の名 {name} ")) && got.contains("Tier と数字の形でない"), "{name}: {got}");
        assert!(got.ends_with(" line=12"), "{name}: 群の行: {got}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// (n) Tier1, Tier2, Tier3 は通る（数字の昇順）。群は宣言順のまま。
#[test]
fn host_group_tier_ascending_numbers_pass_compared_numerically() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let body = format!(
        "{GROUP_HEAD}{}{}{}",
        seed_group("Tier1", "/repo/a", "[\"g1\"]"),
        seed_group("Tier2", "/repo/b", "[\"g2\"]"),
        seed_group("Tier3", "/repo/c", "[\"g3\"]")
    );
    let outcome = group_validate(dir.as_path(), &body);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert!(outcome.err.is_empty(), "欠陥 0: {outcome:?}");
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let names: Vec<&str> = manifest.groups().iter().map(|group| group.name()).collect();
    assert_eq!(names, ["Tier1", "Tier2", "Tier3"], "宣言順のまま（数字で並べ替えない）");
    std::fs::remove_dir_all(&dir).ok();
}

// ─── 群の表の名は Tier1〜Tier9（account-lifecycle.md §34 の行 x・FR38・ADR-0091・接頭辞 `host_group_park_gate_`） ───

/// 「9 を越える」の欠陥の行（名・群の見出し行の番号）。
fn over_nine(name: &str, line: usize) -> String {
    format!("rules: host.toml: 群の名 {name} の数字が 9 を越える（群の表の名は Tier1〜Tier9） line={line}")
}

/// 「park の区画」の欠陥の行（群の見出し行の番号）。
fn park_lot(line: usize) -> String {
    format!("rules: host.toml: Tier9 は park の区画の名で、この版の器は park の区画を読めない line={line}")
}

/// (a) Tier1・Tier10 と Tier1・Tier12 は 2 番目の群の見出し行（17 行目）の「9 を越える」の欠陥 1 件ずつ。base は通る（RED）。
#[test]
fn host_group_park_gate_names_over_nine_are_refused_on_the_group_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    for name in ["Tier10", "Tier12"] {
        let err = tier_refusals(&dir, &[("Tier1", "g1"), (name, "g2")]);
        assert_eq!(err, vec![over_nine(name, 17)], "{name}: 9 を越える 1 件だけ");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// (b) Tier1・Tier9 は 2 番目の群の見出し行（17 行目）の「park の区画」の欠陥 1 件。base は通る（RED）。
#[test]
fn host_group_park_gate_tier9_is_refused_as_a_park_lot() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("Tier1", "g1"), ("Tier9", "g2")]);
    assert_eq!(err, vec![park_lot(17)], "park の区画の 1 件だけ");
    std::fs::remove_dir_all(&dir).ok();
}

/// (c) Tier1〜Tier8 の 8 行は通り、群は宣言順の 8 つ（回帰の歯・base でも緑＝断りを Tier8 以下へ広げる変異を落とす）。
#[test]
fn host_group_park_gate_tier1_to_tier8_pass() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let want: Vec<String> = (1..=8).map(|at| format!("Tier{at}")).collect();
    let head = (4..=8).fold(GROUP_HEAD.to_owned(), |head, at| format!("{head}\n[[account]]\nlabel = \"g{at}\"\n"));
    let body = want.iter().zip(1..).fold(head, |body, (name, at)| {
        format!("{body}{}", seed_group(name, &format!("/repo/{at}"), &format!("[\"g{at}\"]")))
    });
    let outcome = group_validate(dir.as_path(), &body);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert!(outcome.err.is_empty(), "欠陥 0: {outcome:?}");
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let names: Vec<&str> = manifest.groups().iter().map(|group| group.name()).collect();
    assert_eq!(names, want, "宣言順の 8 つ");
    std::fs::remove_dir_all(&dir).ok();
}

/// (d) Tier9・Tier10 はそれぞれの断りの 2 件だけ（Tier10 の行に昇順の欠陥を重ねない）。base は通る（RED）。
#[test]
fn host_group_park_gate_tier9_then_tier10_are_two_refusals() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("Tier9", "g1"), ("Tier10", "g2")]);
    assert_eq!(err, vec![park_lot(12), over_nine("Tier10", 17)], "それぞれの断りだけ");
    std::fs::remove_dir_all(&dir).ok();
}

/// (e) Tier10・Tier3 と Tier9・Tier3 は断った行の 1 件だけ（断った行は前の群にならない＝Tier3 の行の 17 行目の欠陥は 0 件）。
/// base は Tier3 の行に昇順の欠陥が付く（RED）。
#[test]
fn host_group_park_gate_refused_row_is_not_the_prior_group() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("Tier10", "g1"), ("Tier3", "g2")]);
    assert_eq!(err, vec![over_nine("Tier10", 12)], "Tier10 の行の 1 件だけ");
    let err = tier_refusals(&dir, &[("Tier9", "g1"), ("Tier3", "g2")]);
    assert_eq!(err, vec![park_lot(12)], "Tier9 の行の 1 件だけ");
    std::fs::remove_dir_all(&dir).ok();
}

/// host_guard.rm の値の綴り違いの記号は読み込みで拒み（取る記号を名指す）、3 記号は全部受理される。
#[test]
fn rules_host_guard_rm_symbol_misspelled_is_refused_and_the_three_are_accepted() {
    let all: Vec<String> = PROTECTED.iter().map(|symbol| format!("\"{}\"", symbol.as_str())).collect();
    assert_eq!(all.len(), 3, "記号は 3 つ: {all:?}");
    let manifest = parsed(&one_row(RuleKind::HostGuardRmProtected, &format!("[{}]", all.join(", "))))
        .unwrap_or_else(|errors| panic!("3 記号は受理される: {errors}"));
    let row = manifest.get("probe").unwrap_or_else(|| panic!("probe が在る"));
    let want: Vec<String> = ["state-dir", "repo-tracked", "repo-git"].map(str::to_owned).to_vec();
    assert_eq!(row.value, RuleValue::List(want), "宣言順の 3 記号");
    for bad in ["state_dir", "repo-tracked ", "Repo-Git", "home"] {
        let errors = rejected(&one_row(RuleKind::HostGuardRmProtected, &format!("[\"repo-git\", \"{bad}\"]")))
            .unwrap_or_else(|rows| panic!("{bad:?} が受理された（{rows} 行）"));
        let text = errors.join("\n");
        assert!(text.contains(&format!("未知の守る集合の記号 {bad}")), "{bad:?}: {text}");
        assert!(text.contains("state-dir / repo-tracked / repo-git"), "取る記号を名指す: {text}");
    }
}

/// host_guard.publish の要素（設計 vessel-hook.md §16 形 6）: 綴り違いの記号と 63 字の digest は読み込みで拒み（要素を名指す）、
/// 4 記号の `form` と 64 字小文字の `exclude` は受理される。
#[test]
fn rules_publish_guard_element_misspelled_form_and_short_digest_are_refused() {
    let digest = "0123456789abcdef".repeat(4);
    let good = format!("[\"form repo-name\", \"form object-id\", \"form tracked-path\", \"form ledger-id\", \"exclude {digest}\"]");
    let manifest = parsed(&one_row(RuleKind::HostGuardPublish, &good)).unwrap_or_else(|errors| panic!("受理される: {errors}"));
    assert!(matches!(manifest.get("probe").map(|row| &row.value), Some(RuleValue::List(items)) if items.len() == 5), "5 要素");
    for bad in ["form repo_name".to_owned(), format!("exclude {}", digest.get(1..).unwrap_or_default())] {
        let errors = rejected(&one_row(RuleKind::HostGuardPublish, &format!("[\"{bad}\"]")))
            .unwrap_or_else(|rows| panic!("{bad:?} が受理された（{rows} 行）"));
        assert!(errors.join("\n").contains(&format!("要素 {bad:?} の")), "{bad:?}: {errors:?}");
    }
}
