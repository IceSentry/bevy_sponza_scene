use std::{fs::File, io::Write, num::NonZeroU8};

mod camera_controller;
mod mipmap_generator;

use bevy::{
    core_pipeline::{bloom::BloomSettings, fxaa::Fxaa},
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    tasks::IoTaskPool,
};
use camera_controller::{CameraController, CameraControllerPlugin};
use mipmap_generator::{generate_mipmaps, MipmapGeneratorPlugin, MipmapGeneratorSettings};

use crate::convert::{change_gltf_to_use_ktx2, convert_images_to_ktx2};

mod convert;

pub fn main() {
    let args = &mut std::env::args();
    args.next();
    if let Some(arg) = &args.next() {
        if arg == "--convert" {
            println!("This will take a few minutes");
            convert_images_to_ktx2();
            change_gltf_to_use_ktx2();
        }
    }

    let mut app = App::new();

    app.insert_resource(Msaa { samples: 1 })
        .insert_resource(ClearColor(Color::rgb(1.75, 1.9, 1.99)))
        .insert_resource(AmbientLight {
            color: Color::rgb(1.0, 1.0, 1.0),
            brightness: 0.02,
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    window: WindowDescriptor { ..default() },
                    ..default()
                })
                .set(AssetPlugin {
                    watch_for_changes: true,
                    ..default()
                }),
        )
        .add_plugin(LogDiagnosticsPlugin::default())
        .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(CameraControllerPlugin)
        // Generating mipmaps takes a minute
        .insert_resource(MipmapGeneratorSettings {
            anisotropic_filtering: NonZeroU8::new(16),
            ..default()
        })
        .add_plugin(MipmapGeneratorPlugin)
        // Mipmap generation be skipped if ktx2 is used
        .add_system(generate_mipmaps::<StandardMaterial>)
        .add_startup_system(setup)
        .add_system(proc_scene)
        .add_system(save_scene)
        .add_system(input_scene)
        .add_event::<SaveScene>()
        .add_event::<LoadScene>()
        .add_system(load_scene)
        .register_type::<GrifLight>();

    app.run();
}

#[derive(Component)]
pub struct PostProcScene;

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct GrifLight;

pub fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    println!("Loading models, generating mipmaps");

    // sponza
    commands
        .spawn(SceneBundle {
            scene: asset_server.load("main_sponza/NewSponza_Main_glTF_002.gltf#Scene0"),
            ..default()
        })
        .insert(PostProcScene);

    // curtains
    commands
        .spawn(SceneBundle {
            scene: asset_server.load("PKG_A_Curtains/NewSponza_Curtains_glTF.gltf#Scene0"),
            ..default()
        })
        .insert(PostProcScene);

    // commands.spawn(DynamicSceneBundle {
    //     scene: asset_server.load("scenes/day.scn.ron"),
    //     ..default()
    // });

    // Camera
    commands
        .spawn((
            Camera3dBundle {
                camera: Camera {
                    hdr: true,
                    ..default()
                },
                transform: Transform::from_xyz(-10.5, 1.7, -1.0)
                    .looking_at(Vec3::new(0.0, 3.5, 0.0), Vec3::Y),
                projection: Projection::Perspective(PerspectiveProjection {
                    fov: std::f32::consts::PI / 3.0,
                    near: 0.1,
                    far: 1000.0,
                    aspect_ratio: 1.0,
                }),
                ..default()
            },
            BloomSettings {
                threshold: 0.1,
                knee: 0.1,
                scale: 1.0,
                intensity: 0.01,
            },
        ))
        .insert(CameraController::default().print_controls())
        .insert(Fxaa::default());
}

struct SaveScene;
struct LoadScene;

fn input_scene(
    keyboard_input: Res<Input<KeyCode>>,
    mut save_scene_events: EventWriter<SaveScene>,
    mut load_scene_events: EventWriter<LoadScene>,
) {
    if keyboard_input.just_pressed(KeyCode::Y) {
        save_scene_events.send(SaveScene);
    }
    if keyboard_input.just_pressed(KeyCode::H) {
        load_scene_events.send(LoadScene);
    }
}

fn load_scene(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    events: EventReader<LoadScene>,
) {
    if !events.is_empty() {
        events.clear();
        info!("Loading nigth scene");
        commands.spawn(DynamicSceneBundle {
            scene: asset_server.load("scenes/night.scn.ron"),
            ..default()
        });
    }
}

fn save_scene(world: &mut World) {
    let mut q = world.resource_mut::<Events<SaveScene>>();
    if !q.is_empty() {
        q.clear();

        info!("Saving scene");

        let mut scene_world = World::new();

        for i in 0..26 {
            scene_world.spawn(PointLightBundle {
                point_light: PointLight {
                    color: Color::YELLOW,
                    ..Default::default()
                },
                transform: Transform::from_xyz(i as f32, 5.0, 0.0),
                ..Default::default()
            });
        }

        let type_registry = world.resource::<AppTypeRegistry>();
        let scene = DynamicScene::from_world(&scene_world, type_registry);

        let serialized_scene = scene.serialize_ron(type_registry).unwrap();

        IoTaskPool::get()
            .spawn(async move {
                File::create("assets/scenes/night.scn.ron")
                    .and_then(|mut file| file.write(serialized_scene.as_bytes()))
                    .expect("Error while writing scene to file");
                info!("Saving scene done");
            })
            .detach();
    }
}

pub fn all_children<F: FnMut(Entity)>(
    children: &Children,
    children_query: &Query<&Children>,
    closure: &mut F,
) {
    for child in children {
        if let Ok(children) = children_query.get(*child) {
            all_children(children, children_query, closure);
        }
        closure(*child);
    }
}

#[allow(clippy::type_complexity)]
pub fn proc_scene(
    mut commands: Commands,
    flip_normals_query: Query<Entity, With<PostProcScene>>,
    children_query: Query<&Children>,
    has_std_mat: Query<&Handle<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lights: Query<
        Entity,
        (
            Or<(With<PointLight>, With<DirectionalLight>, With<SpotLight>)>,
            Without<GrifLight>,
        ),
    >,
    cameras: Query<Entity, With<Camera>>,
) {
    for entity in flip_normals_query.iter() {
        if let Ok(children) = children_query.get(entity) {
            all_children(children, &children_query, &mut |entity| {
                // Sponza needs flipped normals
                if let Ok(mat_h) = has_std_mat.get(entity) {
                    if let Some(mat) = materials.get_mut(mat_h) {
                        mat.flip_normal_map_y = true;
                    }
                }

                // Sponza has a bunch of lights by default
                if lights.get(entity).is_ok() {
                    commands.entity(entity).despawn_recursive();
                }

                // Sponza has a bunch of cameras by default
                if cameras.get(entity).is_ok() {
                    commands.entity(entity).despawn_recursive();
                }
            });
            commands.entity(entity).remove::<PostProcScene>();
        }
    }
}
