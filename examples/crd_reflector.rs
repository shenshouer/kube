use futures::{StreamExt, TryStreamExt};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use tracing::*;

use kube::{
    api::{Api, Patch, PatchParams, ResourceExt},
    runtime::{reflector, watcher, WatchStreamExt},
    Client, CustomResource, CustomResourceExt,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(group = "clux.dev", version = "v1", kind = "Foo", namespaced)]
pub struct FooSpec {
    name: String,
    info: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let client = Client::try_default()?;

    // 0. Ensure the CRD is installed (you probably just want to do this on CI)
    // (crd file can be created by piping `Foo::crd`'s yaml ser to kubectl apply)
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    info!("Creating crd: {}", serde_yaml::to_string(&Foo::crd())?);
    let ssapply = PatchParams::apply("crd_reflector_example").force();
    crds.patch("foos.clux.dev", &ssapply, &Patch::Apply(Foo::crd()))
        .await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await; // wait for k8s to deal with it

    // 1. Run a reflector against the installed CRD
    let (reader, writer) = reflector::store::<Foo>();

    let foos: Api<Foo> = Api::default_namespaced(client);
    let wc = watcher::Config::default().any_semantic();
    let mut stream = watcher(foos, wc)
        .default_backoff()
        .reflect(writer)
        .applied_objects()
        .boxed();

    tokio::spawn(async move {
        reader.wait_until_ready().await.unwrap();
        loop {
            // Periodically read our state
            // while this runs you can kubectl apply -f crd-baz.yaml or crd-qux.yaml and see it works
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let crds = reader.state().iter().map(|r| r.name_any()).collect::<Vec<_>>();
            info!("Current crds: {:?}", crds);
        }
    });
    while let Some(event) = stream.try_next().await? {
        info!("saw {}", event.name_any());
    }
    Ok(())
}
