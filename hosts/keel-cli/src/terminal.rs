use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    CancelToken, GrantScope, InteractionKind, InteractionPort, InteractionRequest,
    InteractionResponse,
};
use tokio::io::{AsyncBufReadExt, BufReader};

const GRANT_TTL: Duration = Duration::from_secs(8 * 60 * 60);

/// Renders kernel interactions on stderr and reads an answer from stdin.
///
/// A piped or non-terminal stdin never blocks: it defers immediately, so CI
/// and headless runs deny permission-gated actions.
pub struct TerminalInteractionPort;

#[async_trait]
impl InteractionPort for TerminalInteractionPort {
    async fn request(
        &self,
        request: InteractionRequest,
        cancel: &CancelToken,
    ) -> InteractionResponse {
        if !std::io::stdin().is_terminal() {
            return InteractionResponse::Deferred;
        }

        eprintln!();
        eprintln!("[{}] {}", request.kind.as_str(), request.prompt);
        for (index, option) in request.options.iter().enumerate() {
            eprintln!("  {}. {}", index + 1, option.label);
        }
        eprint!("answer (number, text, or Enter to defer): ");
        let _ = std::io::stderr().flush();

        let mut line = String::new();
        let mut reader = BufReader::new(tokio::io::stdin());
        tokio::select! {
            _ = cancel.cancelled() => InteractionResponse::Cancelled,
            result = reader.read_line(&mut line) => {
                match result {
                    Ok(_) => parse_response(&request, line.trim()),
                    Err(_) => InteractionResponse::Deferred,
                }
            }
        }
    }
}

pub(crate) fn parse_response(request: &InteractionRequest, input: &str) -> InteractionResponse {
    if input.is_empty() {
        return InteractionResponse::Deferred;
    }

    let option = selected_option(request, input);
    if request.kind == InteractionKind::PermissionApproval {
        let Some(option) = option else {
            return InteractionResponse::Rejected {
                reason: input.to_owned(),
            };
        };
        return match option.id.as_str() {
            "deny" => InteractionResponse::Rejected {
                reason: "user denied".to_owned(),
            },
            "allow_once" => InteractionResponse::Approved {
                scope: GrantScope::Once,
            },
            "allow_scope" => InteractionResponse::Approved {
                scope: GrantScope::PathPrefix {
                    prefix: request.path.clone().unwrap_or_else(|| PathBuf::from(".")),
                    ttl: GRANT_TTL,
                },
            },
            "full_auto" => InteractionResponse::Approved {
                scope: GrantScope::FullAutoSession { ttl: GRANT_TTL },
            },
            other => InteractionResponse::Selected {
                option: other.to_owned(),
            },
        };
    }

    match option {
        Some(option) => InteractionResponse::Selected {
            option: option.id.clone(),
        },
        None => InteractionResponse::Freeform {
            text: input.to_owned(),
        },
    }
}

fn selected_option<'a>(
    request: &'a InteractionRequest,
    input: &str,
) -> Option<&'a keel_core::InteractionOption> {
    if let Ok(index) = input.parse::<usize>() {
        if (1..=request.options.len()).contains(&index) {
            return request.options.get(index - 1);
        }
    }
    request
        .options
        .iter()
        .find(|option| option.id.eq_ignore_ascii_case(input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::{InteractionOption, TurnId};

    fn permission_request() -> InteractionRequest {
        InteractionRequest::new(
            TurnId::new(1),
            InteractionKind::PermissionApproval,
            "Allow `shell` to execute `cargo`?",
        )
        .with_options(vec![
            InteractionOption::new("deny", "Deny"),
            InteractionOption::new("allow_once", "Allow once"),
            InteractionOption::new("allow_scope", "Allow this path"),
            InteractionOption::new("full_auto", "Full auto"),
        ])
    }

    #[test]
    fn numeric_answers_select_permission_scopes() {
        let request = permission_request();

        assert_eq!(
            parse_response(&request, "2"),
            InteractionResponse::Approved {
                scope: GrantScope::Once
            }
        );
        assert_eq!(
            parse_response(&request, "3"),
            InteractionResponse::Approved {
                scope: GrantScope::PathPrefix {
                    prefix: PathBuf::from("."),
                    ttl: GRANT_TTL
                }
            }
        );
        assert_eq!(
            parse_response(&request, "4"),
            InteractionResponse::Approved {
                scope: GrantScope::FullAutoSession { ttl: GRANT_TTL }
            }
        );
        assert!(matches!(
            parse_response(&request, "1"),
            InteractionResponse::Rejected { .. }
        ));
    }

    #[test]
    fn freeform_answers_apply_to_questions_not_permissions() {
        let request =
            InteractionRequest::new(TurnId::new(1), InteractionKind::Clarify, "Which file?");
        assert_eq!(
            parse_response(&request, "src/main.rs"),
            InteractionResponse::Freeform {
                text: "src/main.rs".to_owned()
            }
        );
        assert_eq!(
            parse_response(&permission_request(), ""),
            InteractionResponse::Deferred
        );
    }
}
