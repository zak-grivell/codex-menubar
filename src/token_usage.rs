use chrono::{Datelike, Days, NaiveDate};
use serde::Deserialize;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TokenTotals {
    pub calendar: Option<CalendarTotals>,
    pub all_time: Option<u128>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CalendarTotals {
    pub today: u128,
    pub week: u128,
    pub month: u128,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    summary: Option<TokenSummary>,
    daily_usage_buckets: Option<Vec<DailyBucket>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenSummary {
    lifetime_tokens: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DailyBucket {
    start_date: String,
    tokens: u64,
}

impl TokenUsage {
    pub fn totals(self, today: NaiveDate) -> Result<TokenTotals, String> {
        let all_time = self
            .summary
            .and_then(|summary| summary.lifetime_tokens)
            .map(u128::from);
        let Some(buckets) = self.daily_usage_buckets else {
            return Ok(TokenTotals {
                calendar: None,
                all_time,
            });
        };
        let week_start = today - Days::new(today.weekday().num_days_from_monday().into());
        let month_start = today.with_day(1).expect("valid first day of the month");
        let mut totals = CalendarTotals::default();
        for bucket in buckets {
            let day = NaiveDate::parse_from_str(&bucket.start_date, "%Y-%m-%d")
                .map_err(|_| "Codex returned an invalid token activity date")?;
            if day > today {
                continue;
            }
            let tokens = u128::from(bucket.tokens);
            if day == today {
                totals.today += tokens;
            }
            if day >= week_start {
                totals.week += tokens;
            }
            if day >= month_start {
                totals.month += tokens;
            }
        }
        Ok(TokenTotals {
            calendar: Some(totals),
            all_time,
        })
    }
}

pub fn format_count(count: u128) -> String {
    if count < 1_000 {
        return count.to_string();
    }

    const UNITS: [&str; 12] = [
        "thousand",
        "million",
        "billion",
        "trillion",
        "quadrillion",
        "quintillion",
        "sextillion",
        "septillion",
        "octillion",
        "nonillion",
        "decillion",
        "undecillion",
    ];

    let mut exponent = count.ilog10();
    let divisor = 10_u128.pow(exponent - 2);
    let mut significant = count / divisor + u128::from(count % divisor >= divisor / 2);
    if significant == 1_000 {
        significant = 100;
        exponent += 1;
    }

    let unit = UNITS[(exponent / 3 - 1) as usize];
    match exponent % 3 {
        0 => format!("{}.{:02} {unit}", significant / 100, significant % 100),
        1 => format!("{}.{} {unit}", significant / 10, significant % 10),
        _ => format!("{significant} {unit}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_three_significant_figures_across_unit_boundaries() {
        for (count, expected) in [
            (0, "0"),
            (27, "27"),
            (999, "999"),
            (1_000, "1.00 thousand"),
            (12_345, "12.3 thousand"),
            (999_499, "999 thousand"),
            (999_500, "1.00 million"),
            (1_995_000, "2.00 million"),
            (9_995_000, "10.0 million"),
            (123_456_789, "123 million"),
            (1_234_567_890, "1.23 billion"),
            (999_500_000_000, "1.00 trillion"),
            (u128::MAX, "340 undecillion"),
        ] {
            assert_eq!(format_count(count), expected);
        }
    }

    #[test]
    fn groups_sparse_daily_buckets_by_week_and_month_boundaries() {
        let buckets = json!({"dailyUsageBuckets": [
            {"startDate": "2026-10-25", "tokens": 100},
            {"startDate": "2026-10-26", "tokens": 200},
            {"startDate": "2026-11-01", "tokens": 300},
            {"startDate": "2026-11-03", "tokens": 400}
        ]});
        let totals = |day| {
            serde_json::from_value::<TokenUsage>(buckets.clone())
                .unwrap()
                .totals(NaiveDate::from_ymd_opt(2026, 11, day).unwrap())
                .unwrap()
                .calendar
                .unwrap()
        };
        assert_eq!(
            totals(1),
            CalendarTotals {
                today: 300,
                week: 500,
                month: 300
            }
        );
        assert_eq!(
            totals(2),
            CalendarTotals {
                today: 0,
                week: 0,
                month: 300
            }
        );
        assert_eq!(
            totals(3),
            CalendarTotals {
                today: 400,
                week: 400,
                month: 700
            }
        );
    }

    #[test]
    fn distinguishes_unavailable_activity_from_an_empty_history() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let lifetime_only: TokenUsage = serde_json::from_value(json!({
            "summary": {"lifetimeTokens": 12_345_678_900_u64},
            "dailyUsageBuckets": null,
        }))
        .unwrap();
        let empty: TokenUsage = serde_json::from_value(json!({
            "summary": {"lifetimeTokens": null},
            "dailyUsageBuckets": [],
        }))
        .unwrap();
        assert_eq!(
            lifetime_only.totals(today).unwrap(),
            TokenTotals {
                calendar: None,
                all_time: Some(12_345_678_900),
            }
        );
        assert_eq!(
            empty.totals(today).unwrap(),
            TokenTotals {
                calendar: Some(CalendarTotals::default()),
                all_time: None,
            }
        );
    }
}
