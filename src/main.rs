extern crate sdl2;
extern crate vulkano;

use image::{ImageBuffer, Rgba};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::device::{Device, DeviceCreateInfo, QueueCreateInfo, QueueFlags};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{ImageDimensions, StorageImage};
use vulkano::instance::{Instance, InstanceCreateInfo, InstanceExtensions};
use vulkano::memory::allocator::StandardMemoryAllocator;
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryUsage};
use vulkano::pipeline::ComputePipeline;
use vulkano::pipeline::Pipeline;
use vulkano::pipeline::PipelineBindPoint;
use vulkano::sync::{self, GpuFuture};
use vulkano::VulkanLibrary;

const SCREEN_WIDTH: u32 = 1920;
const SCREEN_HEIGHT: u32 = 1080;

fn main() {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem
        .window("Rusty Ray Tracer", SCREEN_WIDTH, SCREEN_HEIGHT)
        .vulkan()
        .build()
        .unwrap();

    let instance_extensions =
        InstanceExtensions::from_iter(window.vulkan_instance_extensions().unwrap());

    let library = VulkanLibrary::new().expect("no local Vulkan library/DLL");
    let instance = Instance::new(library, {
        let mut instance_info = InstanceCreateInfo::application_from_cargo_toml();
        instance_info.enabled_extensions = instance_extensions;
        instance_info
    })
    .unwrap();

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
                .contains(QueueFlags::GRAPHICS)
        })
        .expect("couldn't find a graphical queue family") as u32;

    let (device, mut queues) = Device::new(
        physical_device,
        DeviceCreateInfo {
            // here we pass the desired queue family to use by index
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .expect("failed to create device");

    let queue = queues.next().unwrap();

    mod cs {
        vulkano_shaders::shader! {
            ty: "compute",
            path: "src/ray-tracer.comp"
        }
    }

    let shader = cs::load(device.clone()).expect("failed to create shader module");

    let compute_pipeline = ComputePipeline::new(
        device.clone(),
        shader.entry_point("main").unwrap(),
        &(),
        None,
        |_| {},
    )
    .expect("failed to create compute pipeline");

    let memory_allocator = StandardMemoryAllocator::new_default(device.clone());

    let image = StorageImage::new(
        &memory_allocator,
        ImageDimensions::Dim2d {
            width: SCREEN_WIDTH,
            height: SCREEN_HEIGHT,
            array_layers: 1,
        },
        Format::R8G8B8A8_UNORM,
        Some(queue.queue_family_index()),
    )
    .unwrap();
    let view = ImageView::new_default(image.clone()).unwrap();

    let descriptor_set_allocator = StandardDescriptorSetAllocator::new(device.clone());

    let layout = compute_pipeline.layout().set_layouts().get(0).unwrap();
    let set = PersistentDescriptorSet::new(
        &descriptor_set_allocator,
        layout.clone(),
        [WriteDescriptorSet::image_view(0, view)], // 0 is the binding
    )
    .unwrap();

    let buf = Buffer::from_iter(
        &memory_allocator,
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo {
            usage: MemoryUsage::Download,
            ..Default::default()
        },
        (0..SCREEN_HEIGHT * SCREEN_WIDTH * 4).map(|_| 0u8),
    )
    .expect("failed to create buffer");

    let command_buffer_allocator =
        StandardCommandBufferAllocator::new(device.clone(), Default::default());

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
            set,
        )
        .dispatch([SCREEN_WIDTH / 8, SCREEN_HEIGHT / 8, 1])
        .unwrap()
        .copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(image, buf.clone()))
        .unwrap();

    let command_buffer = builder.build().unwrap();

    let future = sync::now(device)
        .then_execute(queue, command_buffer)
        .unwrap()
        .then_signal_fence_and_flush()
        .unwrap();

    future.wait(None).unwrap();

    let buffer_content = buf.read().unwrap();
    let image =
        ImageBuffer::<Rgba<u8>, _>::from_raw(SCREEN_WIDTH, SCREEN_HEIGHT, &buffer_content[..])
            .unwrap();
    image.save("image.png").unwrap();

    println!("Everything succeeded!");

    let mut event_pump = sdl_context.event_pump().unwrap();

    'running: loop {
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
