//! `validate-burn <note.hex> <faucet-id>`
//!
//! Reads a serialized `Note` (hex, as `Note::to_bytes()` writes it) and the USDCx faucet account
//! ID (`0x…` hex or bech32), runs the checks, and prints the withdrawal values.
//!
//! Fetching the note is the caller's job: `GetNotesById(noteId)` on a Miden node returns the
//! public note with its details. Consumption is checked separately with
//! `GetNetworkNoteStatus(noteId)`: only `NullifierCommitted` means the faucet burned it.

use std::process::ExitCode;

use miden_protocol::account::AccountId;
use miden_protocol::note::Note;
use miden_protocol::utils::serde::Deserializable;
use usdcx_burn_validation::validate_burn_note;

fn parse_account_id(text: &str) -> Result<AccountId, String> {
    if text.starts_with("0x") {
        AccountId::from_hex(text).map_err(|error| format!("{error:?}"))
    } else {
        AccountId::from_bech32(text)
            .map(|(network, id)| {
                eprintln!("faucet id given for network {network:?}");
                id
            })
            .map_err(|error| format!("{error:?}"))
    }
}

fn strip_hex_prefix(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, note_path, faucet] = args.as_slice() else {
        eprintln!("usage: validate-burn <note.hex> <faucet-id>");
        return ExitCode::from(2);
    };

    let note = std::fs::read_to_string(note_path)
        .map_err(|error| error.to_string())
        .and_then(|text| hex::decode(strip_hex_prefix(&text)).map_err(|e| e.to_string()))
        .and_then(|bytes| Note::read_from_bytes(&bytes).map_err(|e| format!("{e:?}")));
    let note = match note {
        Ok(note) => note,
        Err(error) => {
            eprintln!("cannot read the note: {error}");
            return ExitCode::from(2);
        }
    };
    let faucet = match parse_account_id(faucet) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("cannot read the faucet id: {error}");
            return ExitCode::from(2);
        }
    };

    match validate_burn_note(&note, faucet) {
        Ok(burn) => {
            println!("valid burn note");
            println!("burnTxId        {}", burn.note_id);
            println!(
                "remoteDepositor 0x{}",
                hex::encode(burn.remote_depositor_bytes32)
            );
            println!("sender (Miden)  {}", burn.remote_depositor.to_hex());
            println!("amount          {} base units", burn.amount);
            println!("destDomain      {}", burn.dest_domain);
            println!("destRecipient   0x{}", hex::encode(burn.dest_recipient));
            println!(
                "next: GetNetworkNoteStatus must report NullifierCommitted before this burn counts"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("rejected: {error}");
            ExitCode::FAILURE
        }
    }
}
