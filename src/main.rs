extern crate sdl2;
extern crate vulkano;

use sdl2::{event::Event, keyboard::Keycode};
use std::sync::Arc;
use vulkano::{
    command_buffer::{
        allocator::StandardCommandBufferAllocator, AutoCommandBufferBuilder, CommandBufferUsage,
    },
    descriptor_set::{
        allocator::StandardDescriptorSetAllocator, PersistentDescriptorSet, WriteDescriptorSet,
    },
    device::{Device, DeviceCreateInfo, DeviceExtensions, QueueCreateInfo, QueueFlags},
    image::{view::ImageView, ImageUsage},
    instance::{Instance, InstanceCreateInfo, InstanceExtensions},
    pipeline::{ComputePipeline, Pipeline, PipelineBindPoint},
    swapchain::{
        AcquireError, Surface, SurfaceApi, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo,
    },
    sync::{FlushError, GpuFuture},
    Handle, VulkanLibrary, VulkanObject,
};

const SCREEN_WIDTH: u32 = 1920;
const SCREEN_HEIGHT: u32 = 1080;

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
            instance.clone(),
            <_ as Handle>::from_raw(surface_handle),
            SurfaceApi::Xlib,
            None,
        )
    };

    // Create a device
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
    let (device, mut queues) = Device::new(
        physical_device.clone(),
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
    let caps = physical_device
        .surface_capabilities(&surface, Default::default())
        .expect("failed to get surface capabilities");

    let composite_alpha = caps.supported_composite_alpha.into_iter().next().unwrap();

    let (swapchain, images) = Swapchain::new(
        device.clone(),
        Arc::new(surface),
        SwapchainCreateInfo {
            min_image_count: caps.min_image_count,
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
    let shader = cs::load(device.clone()).expect("failed to create shader module");

    // Create a compute pipeline to run the shader
    let compute_pipeline = ComputePipeline::new(
        device.clone(),
        shader.entry_point("main").unwrap(),
        &(),
        None,
        |_| {},
    )
    .expect("failed to create compute pipeline");

    let mut event_pump = sdl_context.event_pump().unwrap();

    'running: loop {
        // Acquire the next image from the swapchain
        let (image_index, _suboptimal_acquisition, acquire_future) =
            match vulkano::swapchain::acquire_next_image(swapchain.clone(), None) {
                Ok(result) => result,
                Err(AcquireError::OutOfDate) => {
                    // Recreate swapchain if needed
                    continue;
                }
                Err(err) => panic!("{:?}", err),
            };
        let image = &images[image_index as usize];

        // Define the descriptor set for the compute shader
        let view = ImageView::new_default(image.clone()).unwrap();

        let descriptor_set_allocator = StandardDescriptorSetAllocator::new(device.clone());

        let layout = compute_pipeline.layout().set_layouts().get(0).unwrap();
        let descriptor_set = PersistentDescriptorSet::new(
            &descriptor_set_allocator,
            layout.clone(),
            [WriteDescriptorSet::image_view(0, view)], // 0 is the binding
        )
        .unwrap();

        let command_buffer_allocator =
            StandardCommandBufferAllocator::new(device.clone(), Default::default());
        // Create a command buffer that runs the compute shader and copies the result to the swapchain image
        let mut builder = AutoCommandBufferBuilder::primary(
            &command_buffer_allocator,
            queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();

        builder
            .bind_pipeline_compute(compute_pipeline.clone())
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                compute_pipeline.layout().clone(),
                0,
                descriptor_set,
            )
            .dispatch([SCREEN_WIDTH / 8, SCREEN_HEIGHT / 8, 1])
            .unwrap();

        let command_buffer = builder.build().unwrap();

        // Submit the command buffer to the device queue
        let future = acquire_future
            .then_execute(queue.clone(), command_buffer)
            .unwrap()
            .then_swapchain_present(
                queue.clone(),
                SwapchainPresentInfo::swapchain_image_index(swapchain.clone(), image_index),
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
