//! `<input type=date|time|month|datetime-local|color>`: what each accepts and how the arrow
//! keys step a date or time one field at a time.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Field {
    Year,
    Month,
    Day,
    Hour,
    Minute,
}

/// Where each field sits in the value, as `(field, start, length)` in characters.
pub fn template(kind: &str) -> &'static [(Field, usize, usize)] {
    use Field::*;
    match kind {
        "date" => &[(Year, 0, 4), (Month, 5, 2), (Day, 8, 2)],
        "month" => &[(Year, 0, 4), (Month, 5, 2)],
        "time" => &[(Hour, 0, 2), (Minute, 3, 2)],
        "datetime-local" => &[(Year, 0, 4), (Month, 5, 2), (Day, 8, 2), (Hour, 11, 2), (Minute, 14, 2)],
        _ => &[],
    }
}

pub fn is_temporal(kind: &str) -> bool {
    !template(kind).is_empty()
}

/// May `c` be typed into an input of this type?
pub fn accepts(kind: &str, c: char) -> bool {
    match kind {
        "number" => "0123456789+-.eE".contains(c),
        "color" => c == '#' || c.is_ascii_hexdigit(),
        k if is_temporal(k) => c.is_ascii_digit() || "-:T".contains(c),
        _ => true,
    }
}

pub fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// (year, month, day) of a day count since 1970-01-01 (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i32, i32, i32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((y + i64::from(m <= 2)) as i32, m as i32, d as i32)
}

/// `[year, month, day, hour, minute]` of the current moment (UTC).
pub fn now() -> [i32; 5] {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    [y, m, d, (rem / 3600) as i32, (rem % 3600 / 60) as i32]
}

fn slot(f: Field) -> usize {
    match f {
        Field::Year => 0,
        Field::Month => 1,
        Field::Day => 2,
        Field::Hour => 3,
        Field::Minute => 4,
    }
}

fn parse(kind: &str, value: &str) -> Option<[i32; 5]> {
    let chars: Vec<char> = value.chars().collect();
    let t = template(kind);
    let last = t.last()?;
    if chars.len() != last.1 + last.2 {
        return None;
    }
    let mut out = [1970, 1, 1, 0, 0];
    for &(f, start, len) in t {
        let s: String = chars[start..start + len].iter().collect();
        if !s.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out[slot(f)] = s.parse().ok()?;
    }
    Some(out)
}

fn format(kind: &str, v: [i32; 5]) -> String {
    match kind {
        "date" => format!("{:04}-{:02}-{:02}", v[0], v[1], v[2]),
        "month" => format!("{:04}-{:02}", v[0], v[1]),
        "time" => format!("{:02}:{:02}", v[3], v[4]),
        _ => format!("{:04}-{:02}-{:02}T{:02}:{:02}", v[0], v[1], v[2], v[3], v[4]),
    }
}

/// The field the caret (a character index) is in: the one it touches, else the first.
pub fn field_at(kind: &str, caret: usize) -> Option<Field> {
    let t = template(kind);
    t.iter().find(|(_, s, l)| caret >= *s && caret <= s + l).or(t.first()).map(|f| f.0)
}

/// The value after stepping the field at `caret` up (`dir > 0`) or down. An empty or
/// unparsable value starts from now. Fields wrap (`23:59` -> `00:59`); the day is kept
/// valid for its month.
pub fn step(kind: &str, value: &str, caret: usize, dir: i32) -> Option<String> {
    let field = field_at(kind, caret)?;
    let mut v = parse(kind, value).unwrap_or_else(now);
    let (lo, hi) = match field {
        Field::Year => (1, 9999),
        Field::Month => (1, 12),
        Field::Day => (1, days_in_month(v[0], v[1])),
        Field::Hour => (0, 23),
        Field::Minute => (0, 59),
    };
    let i = slot(field);
    let moved = v[i] + dir.signum();
    v[i] = if moved > hi { lo } else if moved < lo { hi } else { moved };
    v[2] = v[2].min(days_in_month(v[0], v[1]));
    Some(format(kind, v))
}

/// `#rgb` / `#rrggbb` as lowercase `#rrggbb`.
pub fn normalize_color(value: &str) -> Option<String> {
    let c = nile_style::color_from_hex(value.trim().strip_prefix('#')?)?;
    (c.a == 255).then(|| format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_fields_with_wrapping() {
        assert_eq!(step("date", "2024-02-29", 9, 1).unwrap(), "2024-02-01"); // day wraps
        assert_eq!(step("date", "2024-02-29", 9, -1).unwrap(), "2024-02-28");
        assert_eq!(step("date", "2024-01-31", 6, 1).unwrap(), "2024-02-29"); // day clamps to the month
        assert_eq!(step("date", "2024-12-10", 6, 1).unwrap(), "2024-01-10");
        assert_eq!(step("date", "2024-12-10", 2, 1).unwrap(), "2025-12-10");
        assert_eq!(step("time", "23:59", 1, 1).unwrap(), "00:59");
        assert_eq!(step("time", "10:00", 4, -1).unwrap(), "10:59");
        assert_eq!(step("datetime-local", "2024-05-06T07:08", 12, 1).unwrap(), "2024-05-06T08:08");
        assert_eq!(step("month", "2024-05", 5, 1).unwrap(), "2024-06");
        assert!(step("date", "", 0, 1).unwrap().len() == 10);
    }

    #[test]
    fn civil_dates_and_colors() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(normalize_color("#ABC").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_color("#12345g"), None);
        assert!(accepts("color", 'f') && !accepts("color", 'g') && accepts("date", '-') && !accepts("date", 'a'));
    }
}
