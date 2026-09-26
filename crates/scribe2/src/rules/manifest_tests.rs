//! `rules::manifest` の歯。**本体は `manifest.rs`** で、ここには test だけが在る。
//!
//! 分けたのは憲法 C4（1 file の上限）である——`manifest.rs` が 1481 行まで育ち、同じ file へ
//! 行を足す契約を受けられなくなった（`s2-07l.665`）。`#[path]` で `manifest` の子 module として
//! 取り込むので、module path は `rules::manifest::tests` のまま＝歯の名前は 1 つも変わらない。

// 純粋な移動（`manifest.rs` の test 区間から歯を足さずに写した・s2-07l.665）。
// flip-check: moved s2-07l.665

// flip-check: retroactive s2-07l.250
// host の面の読みの 3 値と `labels_over` の歯（設計 account-lifecycle.md §6 / §7・接頭辞 `rules_host_unit_`）。
// 現物の挙動を pin する歯なので base でも通る（`.243` run 3 の生存変異を塞ぐ）。

use super::{collect, finish, Face, HostManifest, Manifest};

/// host の面の本文を読んだ [`HostManifest::Present`]（`[[account]]` を `labels` の順で持つ・見出し行は 3, 6, …）。
fn host_of(labels: &[&str]) -> HostManifest {
    let accounts: String = labels.iter().map(|label| format!("\n[[account]]\nlabel = \"{label}\"\n")).collect();
    match finish(collect(&format!("schema = 1\n{accounts}"), Face::Host)) {
        Ok(face) => HostManifest::Present(face),
        Err(errors) => HostManifest::Unreadable(errors),
    }
}

/// tracked の面の label 列。
fn tracked(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|label| (*label).to_owned()).collect()
}

/// (a) 在るが読めない（dir を `host.toml` の path に渡す＝権限に依らず読めない）は `Unreadable`（`Absent` に潰さない）。
#[test]
fn rules_host_unit_read_of_a_directory_is_unreadable_not_absent() {
    let dir = std::env::temp_dir();
    let read = HostManifest::read(&dir);
    assert_eq!(read.as_str(), "unreadable", "{read:?}");
    let HostManifest::Unreadable(errors) = read else {
        panic!("Unreadable でない");
    };
    assert_eq!(errors.len(), 1, "欠陥 1 件: {errors:?}");
    let first = errors.first().map(ToString::to_string).unwrap_or_default();
    assert!(first.starts_with("rules: host.toml: "), "面の接頭辞: {first}");
    assert!(first.contains(&dir.display().to_string()), "path を名指す: {first}");
    assert!(first.ends_with(" line=0"), "行番号: {first}");
}

/// (b) 無い path だけが `Absent`（縮退・0 宣言）。
#[test]
fn rules_host_unit_read_of_a_missing_path_is_absent() {
    let missing = std::env::temp_dir().join(format!("scribe2-rules-host-unit-missing-{}", std::process::id())).join("host.toml");
    assert_eq!(HostManifest::read(&missing), HostManifest::Absent);
}

/// (c) 重複の無い host の面は tracked の後ろへ宣言順で足す（順序まで）。
#[test]
fn rules_host_unit_labels_over_appends_the_host_labels_in_order() {
    assert_eq!(host_of(&["c"]).labels_over(&tracked(&["a", "b"])), Ok(tracked(&["a", "b", "c"])));
}

/// (d) 面をまたぐ重複は host の面の行番号で 1 件。tracked が重複の label **だけ**の周も同じく拒む
/// （重複判定の `==` を `!=` に倒すと、tracked `[a, b]` では `a` が一致しないことで偽の重複が立ち区別がつかない）。
#[test]
fn rules_host_unit_labels_over_rejects_a_label_crossing_the_faces() {
    for base in [&["a", "b"][..], &["b"]] {
        let errors = host_of(&["b"]).labels_over(&tracked(base)).expect_err("面をまたぐ重複は拒む");
        assert_eq!(errors.len(), 1, "{base:?}: 1 件: {errors:?}");
        let first = errors.first().map(ToString::to_string).unwrap_or_default();
        assert_eq!(first, "rules: host.toml: label b が面をまたいで重複する（tracked の manifest にも在る） line=3", "{base:?}");
    }
}

/// (e) 重複しない label（`d`）は拒まれない＝欠陥はちょうど 1 件。
#[test]
fn rules_host_unit_labels_over_rejects_only_the_crossing_label() {
    let errors = host_of(&["b", "d"]).labels_over(&tracked(&["a", "b"])).expect_err("b は重複する");
    let shown: Vec<(u64, String)> = errors.iter().map(|error| (error.line, error.message.clone())).collect();
    assert_eq!(
        shown,
        vec![(3, "host.toml: label b が面をまたいで重複する（tracked の manifest にも在る）".to_owned())],
        "d は拒まない"
    );
}

// ─── host の面の `[[tick]]`（設計 seat-heartbeat.md §5 形 1・接頭辞 `host_tick_`） ───

/// `body` を host の面の規則で読み、欠陥を (行番号, 文言) の列で返す（読めた周は空）。
fn host_defects(body: &str) -> Vec<(u64, String)> {
    match finish(collect(body, Face::Host)) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.into_iter().map(|error| (error.line, error.message)).collect(),
    }
}

/// 1 行の `[[tick]]` は 2 欄と見出し行を運び（round-trip）、tracked の面に合わせても値が残る。表の無い面は `None`。
#[test]
fn host_tick_one_row_round_trips_its_two_fields_and_line() {
    let body = "schema = 1\n\n[[account]]\nlabel = \"h1\"\n\n[[tick]]\nunit-dir = \"/u/units\"\nbinary = \"/opt/bin/x\"\n";
    let face = finish(collect(body, Face::Host)).unwrap_or_default();
    let tick = face.tick().map(|found| (found.unit_dir().to_owned(), found.binary().to_owned(), found.line()));
    assert_eq!(tick, Some(("/u/units".to_owned(), "/opt/bin/x".to_owned(), 6)), "2 欄と見出し行");
    let joined = Manifest::default().joined(HostManifest::Present(face.clone())).unwrap_or_default();
    assert_eq!(joined.tick(), face.tick(), "面を合わせても同じ値");
    let bare = finish(collect("schema = 1\n\n[[account]]\nlabel = \"h1\"\n", Face::Host)).unwrap_or_default();
    assert_eq!(bare.tick(), None, "表の無い面は None");
    assert_eq!(Manifest::default().joined(HostManifest::Present(bare)).unwrap_or_default().tick(), None);
}

/// 2 行目・相対 path（欄ごと・空の字面も）・欠けた欄・未知 key を行番号つきで 1 件ずつ断り、tracked の面の表は 1 表 1 件で断る。
#[test]
fn host_tick_refuses_a_second_row_relative_paths_and_missing_fields_with_line_numbers() {
    let one = "schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\nbinary = \"/b\"\n";
    assert!(host_defects(one).is_empty(), "1 行は読める");
    let cases: [(String, Vec<(u64, &str)>); 6] = [
        (format!("{one}\n[[tick]]\nunit-dir = \"/v\"\nbinary = \"/c\"\n"), vec![(7, "[[tick]] が重複する（最大 1 行）")]),
        ("schema = 1\n\n[[tick]]\nunit-dir = \"units\"\nbinary = \"/b\"\n".to_owned(), vec![(4, "unit-dir が絶対 path でない: \"units\"")]),
        ("schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\nbinary = \"bin/x\"\n".to_owned(), vec![(5, "binary が絶対 path でない: \"bin/x\"")]),
        ("schema = 1\n\n[[tick]]\nunit-dir = \"\"\nbinary = \"\"\n".to_owned(), vec![(4, "unit-dir が絶対 path でない: \"\""), (5, "binary が絶対 path でない: \"\"")]),
        ("schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\n".to_owned(), vec![(3, "必須 key binary が無い")]),
        ("schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\nbinary = \"/b\"\nperiod = 60\n".to_owned(), vec![(6, "未知の key period")]),
    ];
    for (body, want) in cases {
        let want: Vec<(u64, String)> = want.into_iter().map(|(line, text)| (line, text.to_owned())).collect();
        assert_eq!(host_defects(&body), want, "{body}");
    }
    let tracked = finish(collect("schema = 1\n\n[[tick]]\n", Face::Tracked)).map_err(|errors| {
        errors.into_iter().map(|error| (error.line, error.message)).collect::<Vec<(u64, String)>>()
    });
    assert_eq!(
        tracked,
        Err(vec![(3, "[[tick]] は tracked の manifest に置けない（unit の置き場は host の面だけ）".to_owned())]),
        "tracked の面は 1 表 1 件"
    );
}

// ─── クラスの語列表の読み（設計 contract-source.md §48 の 3・接頭辞 `class_derive_`） ───

/// 語列表の行（見出しは 3 行目・値は `value`）と、上限の行・禁じる語列の 4 行のうち `drop` の id でない行を持つ本文。
fn class_manifest(value: &str, drop: &str) -> String {
    let row = |id: &str, kind: &str, value: &str| {
        format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n")
    };
    let mut text = format!("schema = 1\n{}", row("runner.class_commands", "RunnerClassCommands", value));
    for (id, kind, value) in [
        ("runner.allowed_commands", "RunnerAllowedCommands", "[\"cargo\", \"git\", \"bats\", \"tmux\", \"bd\"]"),
        ("runner.denied_commands", "RunnerDeniedCommands", "[\"cargo mutants\", \"cargo publish\"]"),
        ("host_guard.git", "HostGuardDeniedCommands", "[\"git push --force\", \"git branch -D\"]"),
        ("host_guard.tmux", "HostGuardDeniedCommands", "[\"tmux kill-server\"]"),
        ("host_guard.ledger", "HostGuardDeniedCommands", "[\"bd delete\"]"),
    ] {
        if id != drop {
            text.push_str(&row(id, kind, value));
        }
    }
    text
}

/// 崩れ (a)〜(d) を 1 件ずつ語列表の行の見出しの行番号（3）で断る。(d) は `runner.denied_commands` の語列と host の見張りの
/// 語列 3 行のどれを含む要素でも断る。裁定の 3 要素（禁じる語列 `git push --force` と一部だけ重なる `git push` を含む）は通る。
#[test]
fn class_derive_rules_refuse_each_broken_element_once_with_the_row_line() {
    let ruled = "[\"publish git push\", \"delete git push --delete\", \"delete git push -d\"]";
    assert_eq!(Manifest::parse(&class_manifest(ruled, "")).map(|found| found.rows().len()), Ok(6), "裁定の 3 要素は通る");
    for (value, want) in [
        ("[\"publish git push\", \"ship git push\"]", "先頭語 \"ship\" がクラスの名でない"),
        ("[\"publish git push\", \"consume\"]", "クラス consume の語列が空"),
        ("[\"publish sh push\"]", "語列の先頭語 sh が runner.allowed_commands の値に無い"),
        ("[\"publish cargo publish --dry-run\"]", "禁じる語列 cargo publish を含む"),
        ("[\"delete git push origin --force\"]", "禁じる語列 git push --force を含む"),
        ("[\"delete tmux kill-server\"]", "禁じる語列 tmux kill-server を含む"),
        ("[\"delete bd delete x\"]", "禁じる語列 bd delete を含む"),
    ] {
        let errors = Manifest::parse(&class_manifest(value, "")).expect_err("崩れた要素は断る");
        let shown: Vec<(u64, bool)> = errors.iter().map(|error| (error.line, error.message.contains(want))).collect();
        assert_eq!(shown, vec![(3, true)], "{value}: 1 件・行 3・{want}: {errors:?}");
    }
}

/// 上限の行か禁じる語列の 4 行のどれかを欠く manifest では (c)(d) を撃たない（揃った manifest では同じ要素を断る＝対）。
#[test]
fn class_derive_rules_skip_the_cross_row_checks_without_the_ceiling_or_the_four_denied_rows() {
    let (outside, denied) = ("[\"publish sh push\"]", "[\"delete git push --force\"]");
    for value in [outside, denied] {
        assert!(Manifest::parse(&class_manifest(value, "")).is_err(), "揃った manifest では断る: {value}");
    }
    for drop in ["runner.allowed_commands", "runner.denied_commands", "host_guard.git", "host_guard.tmux", "host_guard.ledger"] {
        for value in [outside, denied] {
            let read = Manifest::parse(&class_manifest(value, drop));
            assert!(read.is_ok(), "{drop} を欠く manifest では撃たない: {value}: {read:?}");
        }
    }
}
