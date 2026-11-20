use chrono::{Datelike, Days, NaiveDate};
use serde::Deserialize;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TokenTotals {
    pub today: u128,
    pub week: u128,
    pub month: u128,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    daily_usage_buckets: Option<Vec<DailyBucket>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DailyBucket {
    start_date: String,
    tokens: u64,
}

impl TokenUsage {
    pub fn totals(self, today: NaiveDate) -> Result<TokenTotals, String> {
        let buckets = self
            .daily_usage_buckets
            .ok_or("Codex did not return daily token activity")?;
        let week_start = today - Days::new(today.weekday().num_days_from_monday().into());
        let month_start = today.with_day(1).expect("valid first day of the month");
        let mut totals = TokenTotals::default();
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
        Ok(totals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        };
        assert_eq!(
            totals(1),
            TokenTotals {
                today: 300,
                week: 500,
                month: 300
            }
        );
        assert_eq!(
            totals(2),
            TokenTotals {
                today: 0,
                week: 0,
                month: 300
            }
        );
        assert_eq!(
            totals(3),
            TokenTotals {
                today: 400,
                week: 400,
                month: 700
            }
        );
    }

    #[test]
    fn distinguishes_unavailable_activity_from_an_empty_history() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let unavailable: TokenUsage =
            serde_json::from_value(json!({"dailyUsageBuckets": null})).unwrap();
        let empty: TokenUsage = serde_json::from_value(json!({"dailyUsageBuckets": []})).unwrap();
        assert!(unavailable.totals(today).is_err());
        assert_eq!(empty.totals(today).unwrap(), TokenTotals::default());
    }
}
