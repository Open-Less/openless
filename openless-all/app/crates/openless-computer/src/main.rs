mod native;
mod protocol;

use std::io::{self, Read, Write};
use std::process::ExitCode;

use protocol::{Error, MAX_REQUEST_BYTES, Request, Result, parse_request};
use serde_json::{Value, json};

fn request_from_cli() -> Result<Request> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--capabilities") if args.next().is_none() => Ok(Request::Capabilities {}),
        Some("--request") => {
            let json = args
                .next()
                .ok_or_else(|| Error::invalid("--request requires a JSON argument"))?;
            if args.next().is_some() {
                return Err(Error::invalid("Unexpected CLI argument"));
            }
            parse_request(json.as_bytes())
        }
        Some(_) => Err(Error::invalid(
            "Use stdin JSON, --request JSON, or --capabilities",
        )),
        None => {
            let mut bytes = Vec::new();
            io::stdin()
                .take(MAX_REQUEST_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| Error::invalid(format!("Cannot read stdin: {error}")))?;
            parse_request(&bytes)
        }
    }
}

fn response(result: Result<Value>) -> (Value, u8) {
    match result {
        Ok(data) => (json!({"ok":true, "data":data}), 0),
        Err(error) => {
            let status = error.exit_code();
            (json!({"ok":false, "error":error}), status)
        }
    }
}

fn main() -> ExitCode {
    let (value, status) = response(request_from_cli().and_then(native::execute));
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if serde_json::to_writer(&mut out, &value).is_err()
        || out.write_all(b"\n").is_err()
        || out.flush().is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::from(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_and_exit_status_have_a_stable_wire_contract() {
        let (value, status) = response(Err(Error::invalid("bad input")));
        assert_eq!(status, 2);
        assert_eq!(
            value,
            json!({"ok":false,"error":{"code":"invalid_request","message":"bad input"}})
        );
        assert_eq!(
            response(Err(Error::new("input_error", "permission denied"))).1,
            1
        );
        assert_eq!(response(Ok(json!({"done":true}))).1, 0);
    }
}
