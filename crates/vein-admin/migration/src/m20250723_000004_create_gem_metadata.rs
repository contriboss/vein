use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(GemMetadata::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(GemMetadata::Name).string().not_null())
                    .col(ColumnDef::new(GemMetadata::Version).string().not_null())
                    .col(ColumnDef::new(GemMetadata::Platform).string().not_null().default("ruby"))
                    .col(ColumnDef::new(GemMetadata::Summary).string())
                    .col(ColumnDef::new(GemMetadata::Description).string())
                    .col(ColumnDef::new(GemMetadata::Licenses).string())
                    .col(ColumnDef::new(GemMetadata::Authors).string())
                    .col(ColumnDef::new(GemMetadata::Emails).string())
                    .col(ColumnDef::new(GemMetadata::Homepage).string())
                    .col(ColumnDef::new(GemMetadata::DocumentationUrl).string())
                    .col(ColumnDef::new(GemMetadata::ChangelogUrl).string())
                    .col(ColumnDef::new(GemMetadata::SourceCodeUrl).string())
                    .col(ColumnDef::new(GemMetadata::BugTrackerUrl).string())
                    .col(ColumnDef::new(GemMetadata::WikiUrl).string())
                    .col(ColumnDef::new(GemMetadata::FundingUrl).string())
                    .col(ColumnDef::new(GemMetadata::MetadataJson).string())
                    .col(
                        ColumnDef::new(GemMetadata::DependenciesJson)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(GemMetadata::ExecutablesJson).string())
                    .col(ColumnDef::new(GemMetadata::ExtensionsJson).string())
                    .col(
                        ColumnDef::new(GemMetadata::HasNativeExtensions)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GemMetadata::HasEmbeddedBinaries)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(GemMetadata::RequiredRubyVersion).string())
                    .col(ColumnDef::new(GemMetadata::RequiredRubygemsVersion).string())
                    .col(ColumnDef::new(GemMetadata::RubygemsVersion).string())
                    .col(ColumnDef::new(GemMetadata::SpecificationVersion).integer())
                    .col(ColumnDef::new(GemMetadata::BuiltAt).string())
                    .col(ColumnDef::new(GemMetadata::SizeBytes).integer())
                    .col(ColumnDef::new(GemMetadata::Sha256).string())
                    .primary_key(
                        Index::create()
                            .col(GemMetadata::Name)
                            .col(GemMetadata::Version)
                            .col(GemMetadata::Platform),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(GemMetadata::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum GemMetadata {
    Table,
    Name,
    Version,
    Platform,
    Summary,
    Description,
    Licenses,
    Authors,
    Emails,
    Homepage,
    DocumentationUrl,
    ChangelogUrl,
    SourceCodeUrl,
    BugTrackerUrl,
    WikiUrl,
    FundingUrl,
    MetadataJson,
    DependenciesJson,
    ExecutablesJson,
    ExtensionsJson,
    HasNativeExtensions,
    HasEmbeddedBinaries,
    RequiredRubyVersion,
    RequiredRubygemsVersion,
    RubygemsVersion,
    SpecificationVersion,
    BuiltAt,
    SizeBytes,
    Sha256,
}
