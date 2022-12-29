pub struct Scraper(pub String);

impl Scraper {
    pub fn get(&self) -> Option<String> {
        match self.0.contains("announce") {
            true => Some(self.0.replace("announce", "scrape")),
            false => None,
        }
    }
}
