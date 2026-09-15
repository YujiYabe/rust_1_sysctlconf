# sysctl-conf

Linux の `sysctl.conf` と同じ形式のファイルを読み込み、Rust の
`serde_json::Map<String, serde_json::Value>` として扱うための小さなライブラリです。
キーに含まれる `.` は Map の階層区切りとして扱います。
CLI は解析結果を JSON として出力します。

## 対応する文法

- `key = value`
- `key value`
- 空行
- 先頭の空白を除いて `#` または `;` で始まるコメント行
- `sysctl.d` で使われるキー先頭の `-`（Map のキーからは除去）
- 同じキーが複数回現れた場合は最後の値を採用

値に含まれる空白や `#`、`;` はそのまま保持します。

## 使用例

```rust
use sysctl_conf::parse_file;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = parse_file("/etc/sysctl.conf")?;

    if let Some(value) = settings["net"]["ipv4"].get("ip_forward") {
        println!("net.ipv4.ip_forward = {value}");
    }

    Ok(())
}
```

CLI から動作確認することもできます。引数を省略した場合は、プロジェクト
直下にあるサンプルの `sysctl.conf` を読み込みます。

```console
cargo run
cargo run -- /etc/sysctl.conf
```

解析結果はキー順の JSON として標準出力へ表示されます。
ファイルが存在しない場合や構文が不正な場合は標準エラーへ理由を表示し、
終了コード `1` で終了します。

任意の `BufRead` や文字列からも読み込めます。

```rust
use sysctl_conf::{parse_reader, parse_str};
use std::io::Cursor;

let from_string = parse_str("vm.swappiness = 10\n")?;
let from_reader = parse_reader(Cursor::new("kernel.pid_max = 4194304\n"))?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## API

- `parse_file(path)` / `load_file(path)`: 任意パスのファイルをロード
- `parse_reader(reader)`: `BufRead` からロード
- `parse_str(text)`: UTF-8 文字列をParse

構文エラーでは行番号と該当行を含む `ParseError` を返します。ファイルや
Reader の読み込みでは、I/O エラーも表現できる `LoadError` を返します。

## テスト

```console
cargo test
```
