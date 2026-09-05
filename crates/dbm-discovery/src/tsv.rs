use crate::{parse_port_spec, split_host_ports, validate_host};

const MAX_TARGET_ROWS: usize = 100;

/// Parse multi-line targets as either `host\tports` (TSV) or `host:ports` per line.
pub fn parse_targets_tsv(input: &str) -> Result<Vec<(String, String)>, String> {
    let mut targets = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        if targets.len() == MAX_TARGET_ROWS {
            return Err(format!(
                "line {line_number}: TSV input exceeds the maximum of {MAX_TARGET_ROWS} rows"
            ));
        }

        let (host, ports) =
            split_target_line(line).map_err(|error| format!("line {line_number}: {error}"))?;

        let host = validate_host(host).map_err(|error| format!("line {line_number}: {error}"))?;
        parse_port_spec(ports).map_err(|error| format!("line {line_number}: {error}"))?;

        targets.push((host, ports.trim().to_string()));
    }

    Ok(targets)
}

/// Parse multi-line targets leniently: each line is parsed independently and
/// reported as its own `Ok(host, ports)` / `Err(reason)` rather than rejecting
/// the whole batch on the first bad line. Unlike [`parse_targets_tsv`], this is
/// the input for the targets editor's per-row status feedback (how many lines
/// succeeded, failed, or were de-duplicated) instead of a strict all-or-nothing
/// import.
pub fn parse_targets_lines_lenient(input: &str) -> Vec<Result<(String, String), String>> {
    let mut rows = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let row = split_target_line(line)
            .map_err(|error| format!("line {line_number}: {error}"))
            .and_then(|(host, ports)| {
                let host =
                    validate_host(host).map_err(|error| format!("line {line_number}: {error}"))?;
                parse_port_spec(ports).map_err(|error| format!("line {line_number}: {error}"))?;
                Ok((host, ports.trim().to_string()))
            });
        rows.push(row);
    }
    rows
}

fn split_target_line(line: &str) -> Result<(&str, &str), String> {
    if line.contains('\t') {
        let mut columns = line.split('\t');
        let host = columns
            .next()
            .expect("split always yields at least one column");
        let ports = columns
            .next()
            .ok_or_else(|| "expected tab-separated host and ports".to_string())?;
        return Ok((host, ports));
    }

    split_host_ports(line).map_err(|error| {
        if error.contains("expected HOST:PORTS") || error.contains("expected [IPv6]:PORTS") {
            format!("{error}; or host\\tports")
        } else {
            error
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_parses_valid_rows_and_ignores_extra_columns() {
        let rows =
            parse_targets_tsv("192.168.1.10\t5432\tignored\n[::1]\t5432, 5433-5434\n").unwrap();

        assert_eq!(
            rows,
            vec![
                ("192.168.1.10".to_string(), "5432".to_string()),
                ("::1".to_string(), "5432, 5433-5434".to_string()),
            ]
        );
    }

    #[test]
    fn colon_format_parses_host_ports_and_bracketed_ipv6() {
        let rows =
            parse_targets_tsv("db.example.com:5432,5433-5440\n[::1]:5440\n192.0.2.10:5432\n")
                .unwrap();

        assert_eq!(
            rows,
            vec![
                ("db.example.com".to_string(), "5432,5433-5440".to_string()),
                ("::1".to_string(), "5440".to_string()),
                ("192.0.2.10".to_string(), "5432".to_string()),
            ]
        );
    }

    #[test]
    fn mixed_tsv_and_colon_rows() {
        let rows = parse_targets_tsv("a.example.com\t5432\nb.example.com:5433\n").unwrap();

        assert_eq!(
            rows,
            vec![
                ("a.example.com".to_string(), "5432".to_string()),
                ("b.example.com".to_string(), "5433".to_string()),
            ]
        );
    }

    #[test]
    fn tsv_missing_ports_column_reports_source_line() {
        let err = parse_targets_tsv("db.example.com\t5432\nmissing-ports\n").unwrap_err();

        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn unbracketed_ipv6_colon_format_is_rejected() {
        let err = parse_targets_tsv("::1:5432\n").unwrap_err();

        assert!(err.contains("bracketed"), "{err}");
    }

    #[test]
    fn tsv_invalid_host_reports_source_line_and_rejects_whole_batch() {
        let err = parse_targets_tsv("db.example.com\t5432\nbad host\t5433\n").unwrap_err();

        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("host"), "{err}");
    }

    #[test]
    fn tsv_invalid_port_spec_reports_source_line() {
        let err = parse_targets_tsv("db.example.com\t5432\nother.example.com\t0\n").unwrap_err();

        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("port"), "{err}");
    }

    #[test]
    fn lenient_parse_reports_each_line_independently() {
        let rows = parse_targets_lines_lenient(
            "db.example.com\t5432\nbad host\t5433\nother.example.com:0\n[::1]\t5440\n",
        );
        assert_eq!(rows.len(), 4);
        // Valid TSV row.
        assert_eq!(
            rows[0].as_ref().unwrap(),
            &("db.example.com".to_string(), "5432".to_string())
        );
        // Invalid host -> that line alone fails.
        assert!(rows[1].is_err());
        assert!(rows[1].as_ref().unwrap_err().contains("line 2"));
        // Invalid port spec -> that line alone fails.
        assert!(rows[2].is_err());
        assert!(rows[2].as_ref().unwrap_err().contains("line 3"));
        // Valid bracketed IPv6 row.
        assert_eq!(
            rows[3].as_ref().unwrap(),
            &("::1".to_string(), "5440".to_string())
        );
    }

    #[test]
    fn lenient_parse_empty_input_has_no_rows() {
        assert!(parse_targets_lines_lenient("").is_empty());
    }

    #[test]
    fn tsv_rejects_more_than_100_rows() {
        let input = (0..101)
            .map(|index| format!("db{index}.example.com\t5432"))
            .collect::<Vec<_>>()
            .join("\n");

        let err = parse_targets_tsv(&input).unwrap_err();

        assert!(err.contains("100"), "{err}");
        assert!(err.contains("line 101"), "{err}");
    }
}
