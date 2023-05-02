extern crate sdl2;
extern crate vulkano;

use sdl2::{event::Event, keyboard::Keycode};
use std::{collections::BTreeMap, sync::Arc};
use vulkano::{
    buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage},
    command_buffer::{
        allocator::StandardCommandBufferAllocator, AutoCommandBufferBuilder, CommandBufferUsage,
    },
    descriptor_set::{
        allocator::StandardDescriptorSetAllocator,
        layout::{DescriptorSetLayoutBinding, DescriptorSetLayoutCreateInfo, DescriptorType},
        PersistentDescriptorSet, WriteDescriptorSet,
    },
    device::{Device, DeviceCreateInfo, DeviceExtensions, QueueCreateInfo, QueueFlags},
    image::{view::ImageView, ImageUsage},
    instance::{Instance, InstanceCreateInfo, InstanceExtensions},
    memory::allocator::{AllocationCreateInfo, MemoryUsage, StandardMemoryAllocator},
    pipeline::{ComputePipeline, Pipeline, PipelineBindPoint},
    shader::ShaderStages,
    swapchain::{
        AcquireError, Surface, SurfaceApi, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo,
    },
    sync::{FlushError, GpuFuture},
    Handle, VulkanLibrary, VulkanObject,
};

const SCREEN_WIDTH: u32 = 1920;
const SCREEN_HEIGHT: u32 = 1080;

#[derive(Debug, Clone, Copy, BufferContents)]
#[repr(C)]
struct Camera {
    position: [f32; 3],
    view_direction: [f32; 3],
    up: [f32; 3],
}

impl Camera {
    fn new(position: [f32; 3], view_direction: [f32; 3], up: [f32; 3]) -> Self {
        Camera {
            position,
            view_direction,
            up,
        }
    }
}

fn main() {
    let sdl_context = sdl2::init().unwrap();

    // Create a Vulkan enabled SDL2 window
    let window = sdl_context
        .video()
        .unwrap()
        .window("Rusty Ray Tracer", SCREEN_WIDTH, SCREEN_HEIGHT)
        .vulkan()
        .build()
        .unwrap();

    // Create a Vulkan instance
    let instance_extensions =
        InstanceExtensions::from_iter(window.vulkan_instance_extensions().unwrap());
    let instance = Instance::new(VulkanLibrary::new().unwrap(), {
        let mut instance_info = InstanceCreateInfo::application_from_cargo_toml();
        instance_info.enabled_extensions = instance_extensions;
        instance_info
    })
    .unwrap();

    // Create a Vulkan surface
    let surface_handle = window
        .vulkan_create_surface(instance.handle().as_raw() as _)
        .unwrap();
    let surface = unsafe {
        Surface::from_handle(
            Arc::clone(&instance),
            <_ as Handle>::from_raw(surface_handle),
            SurfaceApi::Xlib,
            None,
        )
    };

    // Find a physical device (GPU)
    let physical_device = instance
        .enumerate_physical_devices()
        .expect("could not enumerate devices")
        .next()
        .expect("no devices available");
    let queue_family_index = physical_device
        .queue_family_properties()
        .iter()
        .enumerate()
        .position(|(_queue_family_index, queue_family_properties)| {
            queue_family_properties
                .queue_flags
                .contains(QueueFlags::COMPUTE)
        })
        .expect("couldn't find a compute queue family") as u32;

    // Create a device from the physical device
    let (device, mut queues) = Device::new(
        Arc::clone(&physical_device),
        DeviceCreateInfo {
            // here we pass the desired queue family to use by index
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],
            enabled_extensions: DeviceExtensions {
                khr_swapchain: true,
                ..DeviceExtensions::default()
            },
            ..Default::default()
        },
    )
    .expect("failed to create device");
    let queue = queues.next().unwrap();
    println!(
        "Device created from physical device:\n    {}",
        device.physical_device().properties().device_name
    );

    // Create a swapchain to render to the results of the compute shader
    let surface_capabilities = physical_device
        .surface_capabilities(&surface, Default::default())
        .expect("failed to get surface capabilities");
    let composite_alpha = surface_capabilities
        .supported_composite_alpha
        .into_iter()
        .next()
        .unwrap();
    let (swapchain, images) = Swapchain::new(
        Arc::clone(&device),
        Arc::new(surface),
        SwapchainCreateInfo {
            min_image_count: surface_capabilities.min_image_count,
            image_usage: ImageUsage::STORAGE,
            image_extent: [SCREEN_WIDTH, SCREEN_HEIGHT],
            composite_alpha,
            ..Default::default()
        },
    )
    .unwrap();

    // Define the compute shader
    mod cs {
        // Can use the following command to pre-translate the GLSL shader to SPIR-V
        // glslangvalidator -V .\src\ray-tracer.comp -o .\src\ray-tracer.comp.spv --nsf
        vulkano_shaders::shader! {
            ty: "compute",
            path: "src/ray-tracer.comp"
            // bytes: "src/ray-tracer.comp.spv"
        }
    }
    let shader = cs::load(Arc::clone(&device)).expect("failed to create shader module");

    fn replace_create_info(mut create_info: &mut [DescriptorSetLayoutCreateInfo]) {
        let mut ray_tracing_btree_map = BTreeMap::new();
        ray_tracing_btree_map.insert(
            0,
            DescriptorSetLayoutBinding {
                stages: ShaderStages::COMPUTE,
                ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::StorageImage)
            },
        );

        let mut camera_btree_map = BTreeMap::new();
        camera_btree_map.insert(
            0,
            DescriptorSetLayoutBinding {
                stages: ShaderStages::COMPUTE,
                ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::UniformBuffer)
            },
        );

        let set_layout_create_infos = [
            // The first descriptor set is for the ray traced image buffer
            DescriptorSetLayoutCreateInfo {
                bindings: ray_tracing_btree_map,
                ..Default::default()
            },
            // The second descriptor set is for the camera buffer
            DescriptorSetLayoutCreateInfo {
                bindings: camera_btree_map,
                ..Default::default()
            },
        ];
        let mut set_layout_create_infos_slice = set_layout_create_infos.as_slice();
        create_info = &mut set_layout_create_infos_slice
    }

    // Create a compute pipeline to run the shader
    let compute_pipeline = ComputePipeline::new(
        Arc::clone(&device),
        shader.entry_point("main").unwrap(),
        &(),
        None,
        replace_create_info,
        // |_| (),
    )
    .expect("failed to create compute pipeline");

    // println!(
    //     "Compute Pipeline Set Layouts: {:#?}",
    //     compute_pipeline.layout().set_layouts()
    // );

    // Create an SDL event pump to handle events while the window is open
    let mut event_pump = sdl_context.event_pump().unwrap();

    'running: loop {
        let camera = Camera::new([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);

        // Acquire the next image from the swapchain
        let (image_index, _suboptimal_acquisition, acquire_future) =
            match vulkano::swapchain::acquire_next_image(Arc::clone(&swapchain), None) {
                Ok(result) => result,
                Err(AcquireError::OutOfDate) => {
                    // Recreate swapchain if needed
                    continue;
                }
                Err(err) => panic!("{:?}", err),
            };
        let image = &images[image_index as usize];

        println!("Image Index: {}", image_index);
        // println!("Image: {:#?}", image);

        let command_buffer_allocator =
            StandardCommandBufferAllocator::new(Arc::clone(&device), Default::default());

        // Create the descriptor set for the ray tracing buffer
        let view = ImageView::new_default(Arc::clone(&image)).unwrap();
        let descriptor_set_allocator = StandardDescriptorSetAllocator::new(Arc::clone(&device));
        let layout = compute_pipeline.layout().set_layouts().get(0).unwrap();
        // println!("Layout: {:#?}", layout);

        let descriptor_set = PersistentDescriptorSet::new(
            &descriptor_set_allocator,
            Arc::clone(&layout),
            [WriteDescriptorSet::image_view(0, view)], // 0 is the binding
        )
        .unwrap();

        // Create a buffer builder for the ray tracing image buffer
        let mut builder = AutoCommandBufferBuilder::primary(
            &command_buffer_allocator,
            queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();

        builder
            .bind_pipeline_compute(Arc::clone(&compute_pipeline))
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                Arc::clone(&compute_pipeline.layout()),
                0,
                descriptor_set,
            )
            .dispatch([SCREEN_WIDTH, SCREEN_HEIGHT, 1])
            .unwrap();

        // Create a command buffer that runs the compute shader and copies the result to the swapchain image
        let command_buffer = builder.build().unwrap();

        // Create camera buffer
        let camera_buffer_memory_allocator =
            StandardMemoryAllocator::new_default(Arc::clone(&device));
        let camera_descriptor_set_allocator =
            StandardDescriptorSetAllocator::new(Arc::clone(&device));
        let camera_buffer = Buffer::from_data(
            &camera_buffer_memory_allocator,
            BufferCreateInfo {
                usage: BufferUsage::UNIFORM_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                usage: MemoryUsage::Upload,
                ..Default::default()
            },
            camera,
        )
        .unwrap();
        let camera_layout = compute_pipeline.layout().set_layouts().get(1).unwrap();
        // println!("Camera Layout: {:#?}", camera_layout);
        let camera_descriptor_set = PersistentDescriptorSet::new(
            &camera_descriptor_set_allocator,
            Arc::clone(&camera_layout),
            [WriteDescriptorSet::buffer(1, camera_buffer)],
        )
        .unwrap();

        // Create a buffer builder for the camera buffer
        let mut camera_buffer_builder = AutoCommandBufferBuilder::primary(
            &command_buffer_allocator,
            queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();
        camera_buffer_builder
            .bind_pipeline_compute(Arc::clone(&compute_pipeline))
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                Arc::clone(&compute_pipeline.layout()),
                0,
                camera_descriptor_set,
            )
            .dispatch([1, 1, 1])
            .unwrap();

        // Create camera buffer
        let _camera_buffer = camera_buffer_builder.build().unwrap();

        // Submit the command buffer to the device queue
        let future = acquire_future
            .then_execute(Arc::clone(&queue), command_buffer)
            .unwrap()
            .then_swapchain_present(
                Arc::clone(&queue),
                SwapchainPresentInfo::swapchain_image_index(Arc::clone(&swapchain), image_index),
            )
            .then_signal_fence_and_flush()
            .unwrap();

        // Handle any error that occurs during the submission process
        match future.wait(None) {
            Ok(()) => (),
            Err(FlushError::OutOfDate) => {
                // Recreate swapchain if needed
                continue;
            }
            Err(e) => panic!("Error during swapchain present: {:?}", e),
        }

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    break 'running;
                }
                _ => {}
            }
        }
        ::std::thread::sleep(::std::time::Duration::new(0, 1_000_000_000u32 / 60));
    }
}
