//! Helpers for the custom `reader` URI protocol.

/// Empty `reader` protocol response for a status with no body (400/404/500).
pub(crate) fn reader_status(status: tauri::http::StatusCode) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .body(Vec::new())
        .unwrap_or_else(|e| panic!("reader status is a valid response: {e}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn reader_status_carries_the_given_status_code_and_no_body() {
        let response = reader_status(tauri::http::StatusCode::NOT_FOUND);

        assert_eq!(response.status(), tauri::http::StatusCode::NOT_FOUND);
        assert!(response.body().is_empty());
    }
}
