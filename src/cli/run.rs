use ntex::http::StatusCode;

use crate::models::{
  RunArgs, ClusterPartial, ClusterNetworkPartial, CargoPartial,
  ClusterJoinPartial,
};

use crate::client::Nanocld;
use crate::client::error::NanocldError;

use super::errors::CliError;
use super::cargo_image::exec_create_cargo_image;

pub async fn exec_run(
  client: &Nanocld,
  args: &RunArgs,
) -> Result<(), CliError> {
  if let Err(_err) = client.inspect_cargo_image(&args.image).await {
    exec_create_cargo_image(client, &args.image).await?;
  }
  let cluster = ClusterPartial {
    name: args.cluster.to_owned(),
    proxy_templates: None,
  };
  if let Err(err) = client
    .create_cluster(&cluster, args.namespace.to_owned())
    .await
  {
    if let NanocldError::Api(err) = err {
      if err.status != StatusCode::CONFLICT {
        return Err(CliError::Client(NanocldError::Api(err)));
      }
    } else {
      return Err(CliError::Client(err));
    }
  }
  let cluster_network = ClusterNetworkPartial {
    name: args.network.to_owned(),
  };
  if let Err(err) = client
    .create_cluster_network(
      &args.cluster,
      &cluster_network,
      args.namespace.to_owned(),
    )
    .await
  {
    if let NanocldError::Api(err) = err {
      if err.status != StatusCode::CONFLICT {
        return Err(CliError::Client(NanocldError::Api(err)));
      }
    } else {
      return Err(CliError::Client(err));
    }
  }

  let cargo = CargoPartial {
    name: args.name.to_owned(),
    replicas: Some(1),
    dns_entry: None,
    environnements: None,
    config: crate::models::CargoConfig {
      image: Some(args.image.to_owned()),
      ..Default::default()
    },
  };
  if let Err(err) = client.create_cargo(&cargo, args.namespace.to_owned()).await
  {
    if let NanocldError::Api(err) = err {
      if err.status != StatusCode::CONFLICT {
        return Err(CliError::Client(NanocldError::Api(err)));
      }
    } else {
      return Err(CliError::Client(err));
    }
  }

  let cluster_join = ClusterJoinPartial {
    network: args.network.to_owned(),
    cargo: args.name.to_owned(),
  };
  client
    .join_cluster_cargo(&args.cluster, &cluster_join, args.namespace.to_owned())
    .await?;
  client
    .start_cluster(&args.cluster, args.namespace.to_owned())
    .await?;
  Ok(())
}
