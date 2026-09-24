//! credential と HTTP fetch と JSON の読みの群（設計 docs/design/fleet-usage.md §13・契約表の行 c・`s2-07l.542`）。
//!
//! credential を読んで token を取り（[`token_of`]）、client を子 process で起こして本文を受け取り（[`fetch`]）、
//! 応答の木を窓の行へ写す（[`windows_of`]）群である。`fleet/usage.rs` からの**純移動**で、歯は 1 本も足して
//! いない（親に残る in-file の歯と e2e が従来どおり測る）。外の呼び手は 0 で、親の本体と歯からだけ入る。
//! file の末尾の歯は後の起動の記述の置換（設計 core-boundary.md §9 行 g）が [`fetch`] の起動を測るために足した。
//!
//! 子孫は親の私有 item を `super::` でそのまま見るので、**親側の可視性は 1 語も上げていない**。親が呼ぶ
//! 11 名（本体の 7 名と歯の 4 名）だけが `pub(super)` で、残りはこの module に閉じる。

// flip-check: moved s2-07l.542

use super::{endpoint, BETA, CREDENTIAL_FILE, RC_CLIENT_TIMEOUT, SCOPED_KIND, URL};
use crate::fleet::json_tree::{self, Tree};
use crate::fleet::{Allowance, Measured, Unmeasured, UnmeasuredReason, WindowKind};
use crate::invocation::Invocation;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

/// credential の path。**`<state_dir>/accounts/` を走査しない**（label から 1 本に決まる）。
pub(super) fn credential_path(dir: &Path, label: &str) -> PathBuf {
    dir.join("accounts").join(label).join(CREDENTIAL_FILE)
}

/// credential の本文を読む。不在は `NoCredentials`・読めない形は `ShapeMismatch`。
pub(super) fn read_credential(path: &Path) -> Result<String, UnmeasuredReason> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| UnmeasuredReason::ShapeMismatch),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(UnmeasuredReason::NoCredentials)
        }
        Err(_) => Err(UnmeasuredReason::ShapeMismatch),
    }
}

/// いまの UNIX ミリ秒。
pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// credential の本文から token を取る（`claudeAiOauth.accessToken` と `expiresAt` だけを読む）。
///
/// 墓標（`expiresAt == 0`）は token の有無より先に見る——使わない印の置き場に token が
/// 無いのは当然で、`NoToken` と読むと「置き忘れ」に化ける。
pub(super) fn token_of(text: &str, now_ms: u64) -> Result<String, UnmeasuredReason> {
    let tree = json_tree::parse(text).map_err(|_| UnmeasuredReason::ShapeMismatch)?;
    let oauth = tree
        .get("claudeAiOauth")
        .filter(|found| matches!(found, Tree::Object(_)))
        .ok_or(UnmeasuredReason::ShapeMismatch)?;
    let expires = oauth.get("expiresAt").map(epoch_ms);
    if expires == Some(Some(0)) {
        return Err(UnmeasuredReason::Tombstone);
    }
    let token = match oauth.get("accessToken") {
        None | Some(Tree::Null) => return Err(UnmeasuredReason::NoToken),
        Some(found) => found.as_str().ok_or(UnmeasuredReason::ShapeMismatch)?,
    };
    if token.is_empty() {
        return Err(UnmeasuredReason::NoToken);
    }
    // 制御文字（改行）入りの token は設定行を割って別の行を注入できる形なので読まない。
    if token.chars().any(char::is_control) {
        return Err(UnmeasuredReason::ShapeMismatch);
    }
    if expires.flatten().ok_or(UnmeasuredReason::ShapeMismatch)? < now_ms {
        return Err(UnmeasuredReason::TokenExpired);
    }
    Ok(token.to_owned())
}

/// epoch ms の整数。数でない・整数でない値は `None`。
fn epoch_ms(tree: &Tree) -> Option<u64> {
    match tree {
        Tree::Num(text) => text.parse().ok(),
        _ => None,
    }
}

/// client の argv。**token を載せない**（設定は stdin から `-K -` で読ませる）。
pub(super) fn client_args(timeout_s: u64) -> Vec<String> {
    [
        "-sS",
        "-K",
        "-",
        "--max-time",
        &timeout_s.to_string(),
        "-o",
        "-",
        "-w",
        "\n%{http_code}",
        URL,
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect()
}

/// stdin へ書く curl の設定行。値は二重引用で包み、`\` と `"` を escape する。
pub(super) fn config_of(token: &str) -> String {
    let quoted = token.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "header = \"Authorization: Bearer {quoted}\"\n\
         header = \"anthropic-beta: {BETA}\"\n\
         header = \"Accept: application/json\"\n"
    )
}

/// client を起こして本文を受け取る。stderr は捨てる（中継しない）。
pub(super) fn fetch(client: &str, token: &str, timeout_s: u64) -> Result<String, UnmeasuredReason> {
    let mut child = Invocation::new(client)
        .args(client_args(timeout_s))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| UnmeasuredReason::ClientMissing)?;
    if let Some(mut stdin) = child.stdin.take() {
        // 書けない周（client が先に終わった）は応答の側で理由が付く。
        let _ = stdin.write_all(config_of(token).as_bytes());
    }
    let output = child
        .wait_with_output()
        .map_err(|_| UnmeasuredReason::ClientFailed)?;
    match output.status.code() {
        Some(0) => {}
        Some(RC_CLIENT_TIMEOUT) => return Err(UnmeasuredReason::Timeout),
        _ => return Err(UnmeasuredReason::ClientFailed),
    }
    let text = String::from_utf8(output.stdout).map_err(|_| UnmeasuredReason::BodyUnreadable)?;
    body_of(&text).map(str::to_owned)
}

/// stdout の末尾行を HTTP status として外し、本文を返す。200 以外は `HttpStatus`。
pub(super) fn body_of(stdout: &str) -> Result<&str, UnmeasuredReason> {
    match stdout.rsplit_once('\n') {
        Some((body, status)) if status.trim() == "200" => Ok(body),
        _ => Err(UnmeasuredReason::HttpStatus),
    }
}

/// 応答の木を窓の行へ写す。根が object でなければ口座単位の `ShapeMismatch`。
pub(super) fn windows_of(label: &str, root: &Tree) -> Vec<Allowance> {
    if !matches!(root, Tree::Object(_)) {
        return vec![unmeasured(label, None, None, UnmeasuredReason::ShapeMismatch)];
    }
    let mut rows = vec![
        window_row(label, WindowKind::FiveHour, None, root.get("five_hour")),
        window_row(label, WindowKind::SevenDay, None, root.get("seven_day")),
    ];
    rows.extend(model_rows(label, root.get("limits")));
    rows
}

/// `limits[]` の `weekly_scoped` 要素をモデル別の行へ。要素 0 件なら行なし。
///
/// `limits` そのものが無い・配列でない周は窓 1 つの `ShapeMismatch`（黙って「モデル行なし」に
/// 読み替えない）。`display_name` の無い要素は**その要素だけ** Unmeasured になる。
fn model_rows(label: &str, limits: Option<&Tree>) -> Vec<Allowance> {
    let model_window = Some(WindowKind::SevenDayModel);
    let Some(items) = limits.and_then(Tree::as_array) else {
        return vec![unmeasured(label, model_window, None, UnmeasuredReason::ShapeMismatch)];
    };
    items
        .iter()
        .filter(|item| item.get("kind").and_then(Tree::as_str) == Some(SCOPED_KIND))
        .map(|item| {
            let name = item
                .get("scope")
                .and_then(|scope| scope.get("model"))
                .and_then(|model| model.get("display_name"))
                .and_then(Tree::as_str);
            match name {
                None => unmeasured(label, model_window, None, UnmeasuredReason::ShapeMismatch),
                Some(name) => {
                    window_row(label, WindowKind::SevenDayModel, Some(name.to_owned()), Some(item))
                }
            }
        })
        .collect()
}

/// 窓の値を持つ field。`five_hour` / `seven_day` は `utilization`、`limits[]` の要素は `percent`
/// （要素は `utilization` を持たない実測・持っていても読まない）。
fn value_key(window: WindowKind) -> &'static str {
    match window {
        WindowKind::FiveHour | WindowKind::SevenDay => "utilization",
        WindowKind::SevenDayModel => "percent",
    }
}

/// 窓 1 つ（値の field と `resets_at` を持つ object）を行にする。
fn window_row(label: &str, window: WindowKind, model: Option<String>, node: Option<&Tree>) -> Allowance {
    match node.and_then(|node| reading(node, window)) {
        Some((used_pct, resets_at)) => Allowance::Measured(Measured {
            account: label.to_owned(),
            window,
            model,
            endpoint: endpoint(),
            used_pct,
            resets_at,
        }),
        None => unmeasured(label, Some(window), model, UnmeasuredReason::ShapeMismatch),
    }
}

/// 窓の object から（整数 %・正規化した reset）を読む。どちらかが読めなければ `None`。
///
/// `five_hour` / `seven_day` の窓に限り、`resets_at` が null（または不在）で使用率が **0** の周は
/// 「測れた 0%・reset 未定」として reset 無しで読む（ADR-0024 §2.1）。0 以外を reset 無しで
/// 記録しない（`None`＝ShapeMismatch）。`limits[]` の要素には掛けない。
fn reading(node: &Tree, window: WindowKind) -> Option<(u64, Option<String>)> {
    let used_pct = whole_pct(node.get(value_key(window))?)?;
    match node.get("resets_at") {
        None | Some(Tree::Null) if window != WindowKind::SevenDayModel && used_pct == 0 => Some((used_pct, None)),
        found => Some((used_pct, Some(normalize_resets(found?.as_str()?)?))),
    }
}

/// **すでに % の値**（`2.0` = 2%）を整数 % へ切り捨てる（cap しない）。負数と数でない値は `None`。
///
/// [`Tree::as_pct`] は割合を ×100 して切り捨てる。非負の x で `floor(100x) / 100 == floor(x)`
/// なので、その値を 100 で割れば桁の読みを json_tree と共有したまま % の値を読める
/// （×100 が `u64` を超える巨大な値は `None`＝ShapeMismatch）。
fn whole_pct(value: &Tree) -> Option<u64> {
    value.as_pct().map(|hundredths| hundredths / 100)
}

/// Unmeasured の行を組む。
pub(super) fn unmeasured(
    label: &str,
    window: Option<WindowKind>,
    model: Option<String>,
    reason: UnmeasuredReason,
) -> Allowance {
    Allowance::Unmeasured(Unmeasured {
        account: label.to_owned(),
        window,
        model,
        endpoint: endpoint(),
        reason,
    })
}

/// `resets_at` を UTC の `YYYY-MM-DDTHH:MM:SSZ` にする。`Z` と `+00:00` の両形・小数秒を受理する。
///
/// UTC 以外の offset は受けない（読み替えの規則を持たない・parse 不能と同じ扱い）。
pub(super) fn normalize_resets(text: &str) -> Option<String> {
    let stamp = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix("+00:00"))?;
    let stamp = match stamp.split_once('.') {
        None => stamp,
        Some((head, frac)) if !frac.is_empty() && frac.bytes().all(|b| b.is_ascii_digit()) => head,
        Some(_) => return None,
    };
    let shape = b"0000-00-00T00:00:00";
    let bytes = stamp.as_bytes();
    let fits = bytes.len() == shape.len()
        && bytes.iter().zip(shape.iter()).all(|(found, want)| match want {
            b'0' => found.is_ascii_digit(),
            _ => found == want,
        });
    if !fits {
        return None;
    }
    let in_range = |from: usize, low: u32, high: u32| {
        stamp
            .get(from..from + 2)
            .and_then(|digits| digits.parse::<u32>().ok())
            .is_some_and(|value| (low..=high).contains(&value))
    };
    let valid = in_range(5, 1, 12) && in_range(8, 1, 31) && in_range(11, 0, 23) && in_range(14, 0, 59) && in_range(17, 0, 59);
    valid.then(|| format!("{stamp}Z"))
}

#[cfg(test)]
mod tests {
    use super::{client_args, config_of, fetch};
    use crate::fleet::UnmeasuredReason;
    use crate::pipe::fixture::{exited, scratch, Stub};

    /// client の起動は起動の記述を通る（設計 core-boundary.md §9 行 g・採る形 8）: 記録の program は client・引数は
    /// [`client_args`] と同じ列で token を含まない。stub は spawn を断るので、断りは `ClientMissing` に落ちる。
    #[test]
    fn invocation_fleet_usage_curl_names_the_client_and_args_and_refused_spawn_is_client_missing() {
        let (client, token) = ("/nonexistent-invocation-fleet-usage-client", "tok-invocation-secret");
        let stub = Stub::install(|_| exited(0, b"{}\n200"));
        assert_eq!(fetch(client, token, 9), Err(UnmeasuredReason::ClientMissing), "stub の断りは ClientMissing");
        let calls = stub.calls();
        assert_eq!(calls.len(), 1, "起動は 1 回: {calls:?}");
        let call = calls.first().cloned().unwrap_or_else(|| panic!("記録が無い"));
        assert_eq!(call.program, client, "program は client");
        assert_eq!(call.args, client_args(9), "引数は client_args の列");
        assert!(!call.args.iter().any(|arg| arg.contains(token)), "引数に token を載せない: {:?}", call.args);
    }

    /// 据えない周は cfg(test) の実物が撃つ（fetch の読みは不変）: stdin へ [`config_of`] の逐語を書き、stdout の
    /// 末尾行の 200 を外した本文を返す。rc 非 0 で終わる client は `ClientFailed`。
    #[test]
    fn invocation_fleet_usage_curl_real_writes_config_to_stdin_and_reads_stdout_and_nonzero_rc() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("fleet-usage-curl-real");
        let seen = dir.join("stdin.txt");
        let client = |name: &str, tail: &str| {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\ncat > '{}'\n{tail}", seen.display())).expect("偽の client を書ける");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("実行権を付ける");
            path.display().to_string()
        };
        let answering = client("answering", "printf 'body-text\\n200'\n");
        assert_eq!(fetch(&answering, "tok-real", 5), Ok("body-text".to_owned()), "本文を返す");
        assert_eq!(std::fs::read_to_string(&seen).ok(), Some(config_of("tok-real")), "stdin は config_of の逐語");
        let failing = client("failing", "exit 3\n");
        assert_eq!(fetch(&failing, "tok-real", 5), Err(UnmeasuredReason::ClientFailed), "rc 非 0");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
