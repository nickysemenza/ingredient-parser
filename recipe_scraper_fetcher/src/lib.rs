use std::collections::HashMap;

use recipe_scraper::ScrapeError;
use tracing::error;

mod http_utils;

#[derive(Debug)]
pub struct Fetcher {
    client: reqwest_middleware::ClientWithMiddleware,
    cache: Option<HashMap<String, String>>,
}
impl Fetcher {
    pub fn new() -> Self {
        Fetcher {
            client: http_utils::http_client(),
            cache: None,
        }
    }
    pub fn new_with_cache(m: HashMap<String, String>) -> Self {
        Fetcher {
            client: http_utils::http_client(),
            cache: Some(m),
        }
    }
    #[tracing::instrument(name = "scrape_url")]
    pub async fn scrape_url(
        &self,
        url: &str,
    ) -> Result<recipe_scraper::ScrapedRecipe, ScrapeError> {
        let body = self.fetch_html(url).await?;
        recipe_scraper::scrape(body.as_ref(), url)
    }

    #[tracing::instrument]
    async fn fetch_html(&self, url: &str) -> Result<String, ScrapeError> {
        if let Some(cache) = &self.cache {
            if let Some(cached) = cache.get(url) {
                return Ok(cached.to_string());
            }
        }

        let r = match self
            .client
            .get(url)
            .header("user-agent", "recipe")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(match e {
                    reqwest_middleware::Error::Middleware(e) => panic!("{}", e),
                    reqwest_middleware::Error::Reqwest(e) => ScrapeError::Http(e.to_string()),
                })
            }
        };
        if !r.status().is_success() {
            let err_string = r.error_for_status_ref().unwrap_err().to_string();
            error!(
                "failed to fetch {}: {}",
                url,
                r.text().await.unwrap_or_default()
            );
            let e = Err(ScrapeError::Http(err_string));
            return e;
        }
        Ok(r.text().await.unwrap())
    }
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scrape_errors() {
        assert!(matches!(
            Fetcher::new()
                .scrape_url("https://doesnotresolve.com")
                .await
                .unwrap_err(),
            crate::ScrapeError::Http(_)
        ));

        assert_eq!(
            Fetcher::new_with_cache(HashMap::from([(
                "https://doesnotresolve.com".to_string(),
                "foo".to_string(),
            )]))
            .fetch_html("https://doesnotresolve.com")
            .await
            .unwrap(),
            "foo"
        );
    }
}
