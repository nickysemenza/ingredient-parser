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
                    reqwest_middleware::Error::Middleware(e) => {
                        ScrapeError::Http(format!("middleware error: {e}"))
                    }
                    reqwest_middleware::Error::Reqwest(e) => ScrapeError::Http(e.to_string()),
                })
            }
        };
        if let Err(e) = r.error_for_status_ref() {
            let err_string = e.to_string();
            error!(
                "failed to fetch {}: {}",
                url,
                r.text().await.unwrap_or_default()
            );
            return Err(ScrapeError::Http(err_string));
        }
        r.text()
            .await
            .map_err(|e| ScrapeError::Http(format!("failed to read response body: {e}")))
    }
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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

    #[test]
    fn test_default_fetcher() {
        // Test that Default is implemented correctly
        let fetcher = Fetcher::default();
        // Default should have no cache
        assert!(fetcher.cache.is_none());
    }

    #[test]
    fn test_new_with_cache() {
        let cache = HashMap::from([
            ("url1".to_string(), "html1".to_string()),
            ("url2".to_string(), "html2".to_string()),
        ]);
        let fetcher = Fetcher::new_with_cache(cache);
        assert!(fetcher.cache.is_some());
        assert_eq!(fetcher.cache.as_ref().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_cache_hit() {
        let html_content = r#"<!DOCTYPE html>
<html>
<head>
<script type="application/ld+json">
{
  "name": "Test Recipe",
  "recipeIngredient": ["1 cup flour"],
  "recipeInstructions": []
}
</script>
</head>
<body></body>
</html>"#;

        let fetcher = Fetcher::new_with_cache(HashMap::from([(
            "https://example.com/recipe".to_string(),
            html_content.to_string(),
        )]));

        // This should hit the cache and not make a network request
        let result = fetcher.scrape_url("https://example.com/recipe").await;
        assert!(result.is_ok());
        let recipe = result.unwrap();
        assert_eq!(recipe.name, "Test Recipe");
    }

    #[tokio::test]
    async fn test_cache_miss_with_invalid_url() {
        // Cache has a different URL, so it will miss and try to fetch
        let fetcher = Fetcher::new_with_cache(HashMap::from([(
            "https://other.com".to_string(),
            "content".to_string(),
        )]));

        // This URL is not in the cache, so it will try to fetch and fail
        let result = fetcher.scrape_url("https://doesnotresolve.com").await;
        assert!(result.is_err());
    }
}
