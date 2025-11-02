use anyhow::Result;
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;

use crate::config::Config;

pub struct S3Client {
    client: Client,
    pub config: Config,
}

impl S3Client {
    pub async fn new(config: Config) -> Result<Self> {
        let mut aws_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(config.region.clone()));

        if let Some(profile) = &config.profile {
            aws_config = aws_config.profile_name(profile);
        }

        let sdk_config = aws_config.load().await;
        let client = Client::new(&sdk_config);

        Ok(Self { client, config })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn bucket(&self) -> &str {
        &self.config.bucket
    }
}
