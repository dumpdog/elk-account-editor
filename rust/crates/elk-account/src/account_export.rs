//! Edits the zone block of an ElkRP "Export Account" file (the file ElkRP's File > Import Account reads back).
//! The zone block is: count N, then N entries (zone number, then 32 record bytes, one per line), then 13 zone-group values.
//! A zone missing from the file is added as a new entry in zone-number order. Only the zone lines are touched.

use crate::error::{is_num, Error, Result};
use crate::records::ZoneRecord;
use crate::zone_import::ZoneSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneBlock {
    pub count_line: usize,
    pub count: usize,
}

pub struct AccountExport;

impl AccountExport {
    /// Finds the zone block by its shape: a count, then N entries with ascending zone numbers 1..208, each followed
    /// by 32 byte values. Returns None if no unique match exists.
    pub fn find_zone_block(lines: &[String]) -> Option<ZoneBlock> {
        let mut found: Option<ZoneBlock> = None;
        let mut tie = false;
        let mut i = 0;
        while i + 33 < lines.len() {
            let cur = i;
            i += 1;
            let Some(n) = is_num(&lines[cur], i32::MAX) else {
                continue;
            };
            if !(1..=208).contains(&n) {
                continue;
            }
            let n = n as usize;
            if cur + 1 + 33 * n > lines.len() {
                continue;
            }
            let mut ok = true;
            let mut prev = 0;
            'outer: for k in 0..n {
                let p = cur + 1 + 33 * k;
                match is_num(&lines[p], i32::MAX) {
                    Some(zn) if zn > prev && zn <= 208 => prev = zn,
                    _ => {
                        ok = false;
                        break;
                    }
                }
                for b in 1..=32 {
                    if is_num(&lines[p + b], 255).is_none() {
                        ok = false;
                        break 'outer;
                    }
                }
            }
            if !ok {
                continue;
            }
            // Small counts (1, 2, ...) can match by accident elsewhere in the file, so the longest match wins.
            match found {
                Some(f) if n < f.count => {}
                Some(f) if n == f.count => tie = true,
                _ => {
                    found = Some(ZoneBlock {
                        count_line: cur,
                        count: n,
                    });
                    tie = false;
                }
            }
        }
        if tie {
            None
        } else {
            found
        }
    }

    /// Applies the specs and returns the new file text, plus a human-readable log.
    pub fn apply_zones(text: &str, specs: &[ZoneSpec]) -> Result<(String, Vec<String>)> {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        if lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let mut blk = Self::find_zone_block(&lines).ok_or_else(|| {
            Error::invalid_data(
                "Could not find the zone block in this export file (no unique count + ascending zone numbers + 32-byte pattern).",
            )
        })?;
        let mut log = vec![format!(
            "zone block found at line {}: {} stored zones",
            blk.count_line + 1,
            blk.count
        )];
        let (mut updated, mut added) = (0, 0);

        let mut ordered: Vec<&ZoneSpec> = specs.iter().collect();
        ordered.sort_by_key(|s| s.zone); // stable, like OrderBy
        for s in ordered {
            let mut rec = s.to_record()?;
            let mut entry: Option<usize> = None;
            let mut insert_at = blk.count;
            for e in 0..blk.count {
                let zn: i32 = lines[blk.count_line + 1 + 33 * e].parse().unwrap_or(0);
                if zn == s.zone {
                    entry = Some(e);
                    break;
                }
                if zn > s.zone {
                    insert_at = e;
                    break;
                }
            }
            if let Some(e) = entry {
                let p = blk.count_line + 1 + 33 * e;
                let mut old = ZoneRecord::default();
                for b in 0..32 {
                    old.bytes[b] = crate::error::parse_byte(&lines[p + 1 + b])?;
                }
                rec.bytes[3] = old.bytes[3]; // keep the unused byte as ElkRP wrote it
                for b in 0..32 {
                    lines[p + 1 + b] = rec.bytes[b].to_string();
                }
                log.push(format!(
                    "update zone {:>3}: '{}' => '{}' def {} type {} area {}",
                    s.zone,
                    old.name(),
                    s.name,
                    s.definition,
                    s.type_,
                    s.area
                ));
                updated += 1;
            } else {
                let mut add = vec![s.zone.to_string()];
                add.extend(rec.bytes.iter().map(|b| b.to_string()));
                let at = blk.count_line + 1 + 33 * insert_at;
                lines.splice(at..at, add);
                blk = ZoneBlock {
                    count_line: blk.count_line,
                    count: blk.count + 1,
                };
                lines[blk.count_line] = blk.count.to_string();
                log.push(format!(
                    "add    zone {:>3}: '{}' def {} type {} area {}",
                    s.zone, s.name, s.definition, s.type_, s.area
                ));
                added += 1;
            }
        }
        log.push(format!(
            "{updated} updated, {added} added; zone block now holds {} zones",
            blk.count
        ));
        let mut out = lines.join(eol);
        out.push_str(eol);
        Ok((out, log))
    }
}
