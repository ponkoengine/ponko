use super::*;
use crate::reflect::WorldReflect;
use ponko_ecs::Component;

#[test]
fn protocol_describes_every_registered_builtin_without_sampling_instances() {
    let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ZERO);
    // What `Game::physics` and `Game::physics3d` register, so a component
    // they start reflecting can't go undescribed.
    #[cfg(feature = "physics")]
    crate::physics::start(&mut ctx, glam::Vec2::ZERO);
    #[cfg(feature = "physics3d")]
    crate::physics3d_impl::start(&mut ctx, glam::Vec3::ZERO);
    let response = crate::control::handle(&mut ctx, "capabilities", &json!({})).unwrap();
    let components = response["reflection"]["components"].as_array().unwrap();
    for component in components {
        assert_eq!(component["schema"]["status"], "described", "{component}");
    }
    let listed = |name: &str| components.iter().any(|c| c["name"] == name);
    assert!(!cfg!(feature = "physics") || listed("Collider"));
    assert!(!cfg!(feature = "physics3d") || listed("Vehicle3D"));
    assert!(!cfg!(feature = "3d") || (listed("ModelMaterial") && listed("ModelMaterials")));
    let sprite = response["reflection"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Sprite")
        .unwrap();
    assert_eq!(
        sprite["schema"]["json_schema"]["properties"]["texture"]["anyOf"][0]["type"],
        "string"
    );
}

#[test]
fn custom_type_named_like_builtin_remains_opaque() {
    #[derive(serde::Serialize, serde::Deserialize, Component)]
    struct Velocity(u32);
    let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ZERO);
    ctx.world.reflect::<Velocity>();
    let response = crate::control::handle(&mut ctx, "capabilities", &json!({})).unwrap();
    let component = response["reflection"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Velocity")
        .unwrap();
    assert_eq!(component["schema"]["status"], "opaque");
}

#[cfg(all(feature = "window", feature = "3d"))]
#[test]
fn offscreen_device_uses_the_instance_report_path_and_preserves_gpu_fallbacks() {
    // No adapter (a CI runner without a GPU): a logged skip, or a failure
    // under PONKO_REQUIRE_GPU.
    let Some(test) = crate::render::render_test_support::gpu() else {
        return;
    };
    let gpu = crate::render::capabilities::of_device(&test.info, &test.device, (1, 1));
    let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ZERO);
    ctx.headless = false;
    // Offscreen substitution at the report's GPU input; no surface or window is created.
    let response = report_with_gpu(&ctx, Some(&gpu));
    assert_eq!(response["mode"], "windowed");
    assert_eq!(response["subsystems"]["renderer"]["adapter"], json!(gpu.adapter));
    assert_eq!(response["subsystems"]["renderer"]["gpu"], json!(gpu));
    assert!(response["subsystems"]["renderer"]["adapter_reason"].is_null());
    ctx.headless = true;
    let minimal = crate::control::handle(&mut ctx, "capabilities", &json!({})).unwrap();
    assert_eq!(minimal["mode"], "headless");
    assert_eq!(minimal["subsystems"]["renderer"]["active"], false);
}

#[test]
fn protocol_resource_descriptions_replace_metadata_without_changing_values() {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Score {
        total: u32,
    }
    let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ZERO);
    ctx.world.reflect_resource::<Score>();
    ctx.world.insert_resource(Score { total: 7 });
    let schema = crate::reflect::ValueSchema::object([("total", crate::reflect::ValueSchema::Unsigned)]);
    ctx.world.describe_resource("Score", schema.clone()).unwrap();
    let response = crate::control::handle(&mut ctx, "capabilities", &json!({})).unwrap();
    let resource = response["reflection"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Score")
        .unwrap();
    assert_eq!(resource["schema"]["json_schema"], schema.to_json_schema());
    assert_eq!(ctx.world.resource::<Score>().total, 7);
    assert!(ctx.world.describe_resource("missing", schema).is_err());
    let again = crate::control::handle(&mut ctx, "capabilities", &json!({})).unwrap();
    assert_eq!(again["reflection"], response["reflection"]);
}
