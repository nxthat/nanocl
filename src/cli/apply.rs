use std::path::PathBuf;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use ntex::http::StatusCode;

use crate::client::Nanocld;
use crate::client::error::NanocldError;
use crate::models::{
  ApplyArgs, YmlNamespaceConfig, ClusterPartial, ClusterVarPartial,
  ClusterNetworkPartial, CargoPartial, YmlConfigTypes,
};

use super::utils::get_config_type;
use super::errors::CliError;

/**
 * # Apply namespace
 * Apply a namespace configuration file
 */
async fn apply_namespace(
  namespace: &YmlNamespaceConfig,
  client: &Nanocld,
) -> Result<(), CliError> {
  // Create namespace if not exists
  if client.inspect_namespace(&namespace.name).await.is_err() {
    client.create_namespace(&namespace.name).await?;
  }

  // Create clusters
  namespace
    .clusters
    .iter()
    .map(|cluster| async {
      let cluster_exists = client
        .inspect_cluster(&cluster.name, Some(namespace.name.to_owned()))
        .await;
      let item = ClusterPartial {
        name: cluster.name.to_owned(),
        proxy_templates: cluster.proxy_templates.to_owned(),
      };
      if let Ok(curr_cluster) = cluster_exists {
        println!(" current cluster {:#?} ", curr_cluster);
        if let Some(proxy_templates) = cluster.proxy_templates.to_owned() {
          let cluster_name = &curr_cluster.name;
          let namespace_name = &namespace.name;
          proxy_templates
            .into_iter()
            .filter_map(|template| {
              let is_existing = curr_cluster
                .proxy_templates
                .iter()
                .cloned()
                .find(|user_template| user_template == &template);

              if is_existing.is_none() {
                Some(template)
              } else {
                None
              }
            })
            .map(|proxy| async move {
              client
                .link_proxy_template_to_cluster(
                  cluster_name,
                  &proxy,
                  Some(namespace_name.to_owned()),
                )
                .await?;
              Ok::<_, CliError>(())
            })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<()>, CliError>>()?;
        }
      } else {
        client
          .create_cluster(&item, Some(namespace.name.to_owned()))
          .await?;
      }
      // Create cluster variables
      if let Some(variables) = cluster.variables.to_owned() {
        let variables = &variables;
        variables
          .to_owned()
          .into_keys()
          .map(|var_name| async {
            let result = client
              .inspect_cluster_var(
                &cluster.name,
                &var_name,
                Some(namespace.name.to_owned()),
              )
              .await;
            let value = variables.get(&var_name).unwrap();
            let item = ClusterVarPartial {
              name: var_name,
              value: value.into(),
            };
            if result.is_err() {
              client
                .create_cluster_var(
                  &cluster.name.to_owned(),
                  &item,
                  Some(namespace.name.to_owned()),
                )
                .await?;
            }
            Ok::<_, CliError>(())
          })
          .collect::<FuturesUnordered<_>>()
          .collect::<Vec<_>>()
          .await
          .into_iter()
          .collect::<Result<Vec<()>, CliError>>()?;
      }
      // Create cluster networks
      namespace
        .networks
        .iter()
        .map(|network| async {
          let result = client
            .inspect_cluster_network(
              &cluster.name,
              &network.name,
              Some(namespace.name.to_owned()),
            )
            .await;
          let item = ClusterNetworkPartial {
            name: network.name.to_owned(),
          };
          if result.is_err() {
            client
              .create_cluster_network(
                &cluster.name,
                &item,
                Some(namespace.name.to_owned()),
              )
              .await?;
          }

          Ok::<_, CliError>(())
        })
        .collect::<FuturesUnordered<_>>()
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<()>, CliError>>()?;
      Ok::<_, CliError>(())
    })
    .collect::<FuturesUnordered<_>>()
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<()>, CliError>>()?;

  // Create cargoes
  namespace
    .cargoes
    .iter()
    .map(|cargo| async {
      let result = client
        .inspect_cargo(&cargo.name, Some(namespace.name.to_owned()))
        .await;
      let item = CargoPartial {
        name: cargo.name.to_owned(),
        dns_entry: cargo.dns_entry.to_owned(),
        replicas: cargo.replicas.to_owned(),
        environnements: cargo.environnements.to_owned(),
        config: cargo.config.to_owned(),
      };
      if result.is_err() {
        client
          .create_cargo(&item, Some(namespace.name.to_owned()))
          .await?;
      }
      Ok::<_, CliError>(())
    })
    .collect::<FuturesUnordered<_>>()
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<()>, CliError>>()?;

  namespace
    .clusters
    .iter()
    .map(|cluster| async {
      if let Some(joins) = &cluster.joins {
        joins
          .iter()
          .map(|join| async {
            if let Err(err) = client
              .join_cluster_cargo(
                &cluster.name,
                join,
                Some(namespace.name.to_owned()),
              )
              .await
            {
              if let NanocldError::Api(ref err) = err {
                if err.status == StatusCode::CONFLICT {
                  return Ok::<_, CliError>(());
                }
              }
              return Err(CliError::Client(err));
            }

            Ok::<_, CliError>(())
          })
          .collect::<FuturesUnordered<_>>()
          .collect::<Vec<_>>()
          .await
          .into_iter()
          .collect::<Result<Vec<()>, CliError>>()?;
      }

      if let Some(auto_start) = cluster.auto_start {
        if !auto_start {
          return Ok::<_, CliError>(());
        }
        client
          .start_cluster(&cluster.name, Some(namespace.name.to_owned()))
          .await?;
      }

      Ok::<_, CliError>(())
    })
    .collect::<FuturesUnordered<_>>()
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<()>, CliError>>()?;

  Ok(())
}

async fn apply(file_path: PathBuf, client: &Nanocld) -> Result<(), CliError> {
  let file_content = std::fs::read_to_string(file_path)?;
  let config_type = get_config_type(&file_content)?;
  match config_type {
    YmlConfigTypes::Namespace => {
      let namespace =
        serde_yaml::from_str::<YmlNamespaceConfig>(&file_content)?;
      apply_namespace(&namespace, client).await?;
    }
    _ => todo!("apply different type of config"),
  }
  Ok(())
}

pub async fn exec_apply(
  client: &Nanocld,
  args: &ApplyArgs,
) -> Result<(), CliError> {
  let mut file_path = std::env::current_dir()?;
  file_path.push(&args.file_path);
  apply(file_path, client).await?;
  Ok(())
}
