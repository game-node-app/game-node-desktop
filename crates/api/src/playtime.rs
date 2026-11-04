//! Absolute provider totals, submitted once with no HTTP retries.
use crate::GameNodeClient;
use async_trait::async_trait;
use gamenode_core::{
    SessionCredential,
    playtime::{Submission, SubmitError},
    ports::PlaytimeApi,
};
use serde_json::json;

#[async_trait]
impl PlaytimeApi for GameNodeClient {
    async fn submit(
        &self,
        credential: &SessionCredential,
        total: &Submission,
    ) -> Result<(), SubmitError> {
        let mut body = json!({"gameId":total.game_id,"platformId":6,"source":total.source,"totalPlaytimeSeconds":total.total_seconds});
        if let Some(at) = total.last_played_at {
            body["lastPlayedDate"] = json!(iso_date(at).ok_or(SubmitError::Rejected)?);
        }
        let response = self
            .authorized(self.http.post(self.v1("playtime", |_| {})), credential)
            .json(&body)
            .send()
            .await
            .map_err(|_| SubmitError::Unavailable)?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else if status.as_u16() == 401 {
            Err(SubmitError::Unauthorized)
        } else if status.is_client_error() {
            Err(SubmitError::Rejected)
        } else {
            Err(SubmitError::Unavailable)
        }
    }
}

/// Gregorian civil date from Unix days, with the ISO range restricted to four-digit years.
fn iso_date(seconds: i64) -> Option<String> {
    if !(0..=253402300799).contains(&seconds) {
        return None;
    }
    let z = seconds / 86400 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let day_seconds = seconds % 86400;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        day_seconds / 3600,
        (day_seconds % 3600) / 60,
        day_seconds % 60
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::MockServer;
    use url::Url;
    #[test]
    fn dates_include_leap_days_and_limits() {
        for (seconds, date) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (951782400, "2000-02-29T00:00:00.000Z"),
            (253402300799, "9999-12-31T23:59:59.000Z"),
        ] {
            assert_eq!(iso_date(seconds).as_deref(), Some(date));
        }
        assert_eq!(iso_date(-1), None);
        assert_eq!(iso_date(i64::MAX), None);
    }
    #[tokio::test]
    async fn request_body_auth_and_status_paths() {
        for (status, result) in [
            (201, Ok(())),
            (401, Err(SubmitError::Unauthorized)),
            (400, Err(SubmitError::Rejected)),
            (422, Err(SubmitError::Rejected)),
            (500, Err(SubmitError::Unavailable)),
        ] {
            let server = MockServer::start_async().await;
            let mock=server.mock_async(|when,then| {
                when.method("POST").path("/v1/playtime").header("authorization","Bearer token")
                    .json_body(json!({"gameId":620,"platformId":6,"source":"steam","totalPlaytimeSeconds":45240,"lastPlayedDate":"1970-01-01T00:00:00.000Z"}));
                then.status(status);
            }).await;
            let client = GameNodeClient::new(Url::parse(&server.base_url()).unwrap()).unwrap();
            let credential = SessionCredential::new("token").unwrap();
            assert_eq!(
                client
                    .submit(
                        &credential,
                        &Submission {
                            game_id: 620,
                            source: "steam".into(),
                            total_seconds: 45240,
                            last_played_at: Some(0)
                        }
                    )
                    .await,
                result
            );
            mock.assert_async().await;
        }
    }
}
