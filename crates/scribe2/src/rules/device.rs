//! host の面の端末の表 `[[device]]` の行の組み立てと欄の検査と名の重複の検査（設計 host-init.md §15・ADR-0076）。
//!
//! 表は**host の面にだけ**置ける（端末の値は host 固有で PUBLIC repo に載せない・CON2）。tracked の面に置いた表は
//! [`super::manifest`] の読み手が 1 表 1 件で断る。読み手はここを呼ぶだけで、見出しと受ける key の列もここの定数を引く。
//! 器は端末の値を読んで形を守るだけで使わない（ssh も Chrome も撃たない）。

use super::manifest::{check_keys, list_field, text_field, RawRow, RawValue};
use super::RuleError;

/// 表の見出しの字面。
pub(super) const HEADER: &str = "[[device]]";

/// 1 行が持てる key の全体（必須 4 つ・任意 3 つ）。
pub(super) const KEYS: &[&str] = &["name", "ssh", "chrome", "os", "display", "ime-env", "profile-dir"];

/// 1 行に必ず要る key。
pub(super) const REQUIRED: &[&str] = &["name", "ssh", "chrome", "os"];

/// 端末の OS（閉じた 3 語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// `linux`。
    Linux,
    /// `macos`。
    Macos,
    /// `windows`。
    Windows,
}

/// [`Os`] の全 variant（宣言順）。
const OSES: &[Os] = &[Os::Linux, Os::Macos, Os::Windows];

impl Os {
    /// 宣言の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }

    /// 字面から引く（3 語の外は `None`）。
    fn parse(text: &str) -> Option<Self> {
        OSES.iter().copied().find(|os| os.as_str() == text)
    }
}

/// `[[device]]` 1 行が名乗る端末（host 固有の値・host の面にだけ書く）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    name: String,
    ssh: String,
    chrome: String,
    os: Os,
    display: Option<String>,
    ime_env: Vec<(String, String)>,
    profile_dir: Option<String>,
    line: u64,
}

impl Device {
    /// 端末の名（host の面で一意・空白を含まない 1 語）。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// ssh の宛先（空白を含まない 1 語）。
    pub fn ssh(&self) -> &str {
        &self.ssh
    }

    /// 端末の上の Chrome の path。
    pub fn chrome(&self) -> &str {
        &self.chrome
    }

    /// 端末の OS。
    pub fn os(&self) -> Os {
        self.os
    }

    /// 画面の番号（任意・無ければ `None`）。
    pub fn display(&self) -> Option<&str> {
        self.display.as_deref()
    }

    /// IME の env の (KEY, VALUE) の列を**宣言順**で（任意・無ければ空）。
    pub fn ime_env(&self) -> &[(String, String)] {
        &self.ime_env
    }

    /// 端末の上の専用の profile の dir（任意・無ければ `None`）。
    pub fn profile_dir(&self) -> Option<&str> {
        self.profile_dir.as_deref()
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// `[[device]]` 1 行を組む。欠けや未知 key は全件 `errors` へ積み、他の表と同じ形で打ち切る（読めなかった値は scan が
/// 1 件報告済み・key の欠けと型違いも報告済み）。欄の値の形に外れるものは、その key の行番号で 1 件ずつ断る。
pub(super) fn build(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<Device> {
    let before = errors.len();
    check_keys(raw, errors);
    if raw.fields.iter().any(|(_, value, _)| matches!(value, RawValue::Broken)) {
        return None;
    }
    let name = text_field(raw, "name", errors);
    let ssh = text_field(raw, "ssh", errors);
    let chrome = text_field(raw, "chrome", errors);
    let os = text_field(raw, "os", errors);
    let display = text_field(raw, "display", errors);
    let profile_dir = text_field(raw, "profile-dir", errors);
    let ime_env = list_field(raw, "ime-env", errors).unwrap_or_default();
    let (Some(name), Some(ssh), Some(chrome), Some(os)) = (name, ssh, chrome, os) else {
        return None;
    };
    if errors.len() > before {
        return None;
    }
    for (key, value) in [("name", &name), ("ssh", &ssh)] {
        if let Some(reason) = word_defect(value) {
            errors.push(RuleError::new(line_of(raw, key), format!("{key} {reason}")));
        }
    }
    for (key, value) in [("chrome", Some(&chrome)), ("display", display.as_ref()), ("profile-dir", profile_dir.as_ref())] {
        if value.is_some_and(String::is_empty) {
            errors.push(RuleError::new(line_of(raw, key), format!("{key} が空である")));
        }
    }
    let parsed = Os::parse(&os);
    if parsed.is_none() {
        let words: Vec<&str> = OSES.iter().map(|found| found.as_str()).collect();
        errors.push(RuleError::new(line_of(raw, "os"), format!("os {os:?} が {} のどれでもない", words.join(" / "))));
    }
    let ime_env = env_pairs(&ime_env, line_of(raw, "ime-env"), errors);
    let os = parsed?;
    (errors.len() == before).then_some(Device { name, ssh, chrome, os, display, ime_env, profile_dir, line: raw.line })
}

/// 空白を含まない 1 語でない理由（空・空白を含む）。1 語なら `None`。
fn word_defect(value: &str) -> Option<String> {
    if value.is_empty() {
        Some("が空である".to_owned())
    } else if value.chars().any(char::is_whitespace) {
        Some(format!("が空白を含む: {value:?}"))
    } else {
        None
    }
}

/// key が書かれていた物理行番号（無ければ見出しの行）。
fn line_of(raw: &RawRow, key: &str) -> u64 {
    raw.fields.iter().find(|(found, _, _)| found == key).map_or(raw.line, |(_, _, line)| *line)
}

/// `ime-env` の要素を (KEY, VALUE) へ割る。形に外れる要素と、前の要素と同じ KEY は `line`（`ime-env` の行）で 1 件ずつ断る。
fn env_pairs(items: &[String], line: u64, errors: &mut Vec<RuleError>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for item in items {
        match env_pair(item) {
            Err(reason) => errors.push(RuleError::new(line, format!("ime-env の要素 {item:?} が KEY=VALUE の形でない（{reason}）"))),
            Ok((key, _)) if pairs.iter().any(|(found, _)| *found == key) => {
                errors.push(RuleError::new(line, format!("ime-env の KEY {key} が重複する")));
            }
            Ok(pair) => pairs.push(pair),
        }
    }
    pairs
}

/// `KEY=VALUE` 1 つを最初の `=` で割る。KEY は英大文字と数字と `_` で先頭は数字でない・VALUE は空でない（VALUE の中の `=` は
/// そのまま VALUE に残る）。
fn env_pair(item: &str) -> Result<(String, String), &'static str> {
    let (key, value) = item.split_once('=').ok_or("= が無い")?;
    if key.is_empty() {
        return Err("KEY が空である");
    }
    if key.starts_with(|first: char| first.is_ascii_digit()) {
        return Err("KEY の先頭が数字である");
    }
    if !key.chars().all(|found| found.is_ascii_uppercase() || found.is_ascii_digit() || found == '_') {
        return Err("KEY は英大文字と数字と _ だけ");
    }
    if value.is_empty() {
        return Err("VALUE が空である");
    }
    Ok((key.to_owned(), value.to_owned()))
}

/// 端末の名の重複を、2 度目の行の見出しの行番号で 1 件ずつ断る（同じ名の端末が 2 つ在ると名で引けない）。
pub(super) fn check_names(devices: &[Device], errors: &mut Vec<RuleError>) {
    for (index, device) in devices.iter().enumerate() {
        if devices.iter().take(index).any(|found| found.name == device.name) {
            errors.push(RuleError::new(device.line, format!("端末の名 {} が重複する", device.name)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::manifest::HostManifest;
    use super::{env_pair, Os};

    /// host の面の本文を tmp の file に書いて読み、合わせた後の端末の列を返す（読めなければ欠陥の字面の列）。
    fn read_face(name: &str, body: &str) -> Result<Vec<super::Device>, Vec<String>> {
        let dir = std::env::temp_dir().join(format!("host-device-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|err| vec![err.to_string()])?;
        let path = dir.join("host.toml");
        std::fs::write(&path, body).map_err(|err| vec![err.to_string()])?;
        let read = HostManifest::read(&path);
        std::fs::remove_dir_all(&dir).ok();
        match read {
            HostManifest::Present(face) => Ok(face.devices().to_vec()),
            HostManifest::Unreadable(errors) => Err(errors.iter().map(ToString::to_string).collect()),
            HostManifest::Absent => Err(vec!["absent".to_owned()]),
        }
    }

    /// 1 行の組み立ての round-trip: 書いた 7 欄が読み手の口からそのまま返り（`ime-env` は最初の `=` で割った宣言順の組）、
    /// OS は 3 語とも字面 → 型 → 字面で同じ語に戻る。
    #[test]
    fn host_device_one_row_round_trips_every_field() {
        let body = "schema = 1\n\n[[device]]\nname = \"mac-1\"\nssh = \"me@mac\"\nchrome = \"/Applications/Google Chrome.app\"\nos = \"macos\"\ndisplay = \":1\"\nime-env = [\"GTK_IM_MODULE=fcitx\", \"XMODIFIERS=@im=fcitx\"]\nprofile-dir = \"/tmp/prof\"\n";
        let devices = read_face("round-trip", body).expect("1 行の面が読める");
        assert_eq!(devices.len(), 1, "{devices:?}");
        let device = devices.first().expect("1 行");
        assert_eq!(
            (device.name(), device.ssh(), device.chrome(), device.os(), device.display(), device.profile_dir(), device.line()),
            ("mac-1", "me@mac","/Applications/Google Chrome.app", Os::Macos, Some(":1"), Some("/tmp/prof"), 3)
        );
        let env: Vec<(&str, &str)> = device.ime_env().iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
        assert_eq!(env, [("GTK_IM_MODULE", "fcitx"), ("XMODIFIERS", "@im=fcitx")], "VALUE の中の = は VALUE に残る");
        for os in [Os::Linux, Os::Macos, Os::Windows] {
            assert_eq!(Os::parse(os.as_str()), Some(os), "{os:?}");
        }
        assert_eq!(Os::parse("Linux"), None, "3 語の外（大文字）は引けない");
    }

    /// `ime-env` の形の境界: 先頭の数字・`=` の無い字面・空の KEY・空の VALUE は断り、VALUE の中の `=` と KEY の途中の数字と `_`
    /// は受ける。
    #[test]
    fn host_device_ime_env_form_boundaries() {
        assert_eq!(env_pair("1KEY=v"), Err("KEY の先頭が数字である"));
        assert_eq!(env_pair("KEYV"), Err("= が無い"));
        assert_eq!(env_pair("=v"), Err("KEY が空である"));
        assert_eq!(env_pair("KEY="), Err("VALUE が空である"));
        assert_eq!(env_pair("key=v"), Err("KEY は英大文字と数字と _ だけ"));
        assert_eq!(env_pair("K=a=b"), Ok(("K".to_owned(), "a=b".to_owned())), "最初の = で割る");
        assert_eq!(env_pair("_K2=v"), Ok(("_K2".to_owned(), "v".to_owned())));
    }

    /// 面を通した `ime-env` の断り: 形の外の要素と KEY の重複は `ime-env` の行で 1 件ずつ（形の良い要素は数えない）。
    #[test]
    fn host_device_ime_env_defects_are_one_each_on_the_field_line() {
        let body = "schema = 1\n\n[[device]]\nname = \"a\"\nssh = \"a\"\nchrome = \"/c\"\nos = \"linux\"\nime-env = [\"A=1\", \"9B=2\", \"A=3\"]\n";
        let errors = read_face("ime-env", body).expect_err("断る");
        assert_eq!(
            errors,
            [
                "rules: host.toml: ime-env の要素 \"9B=2\" が KEY=VALUE の形でない（KEY の先頭が数字である） line=8",
                "rules: host.toml: ime-env の KEY A が重複する line=8",
            ]
        );
    }
}
