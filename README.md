# Rusty Ray Tracer
This project was chosen as an exercise primarily to learn [Rust](https://www.rust-lang.org/), but it quickly turned into an exercise in learning about [Vulkan](https://en.wikipedia.org/wiki/Vulkan) and [compute shaders](https://vkguide.dev/docs/gpudriven/compute_shaders/). 
It is intended to be a voxel ray tracer that only uses compute shaders but as of right now it only renders spheres using a standard [line-sphere intersection](https://en.wikipedia.org/wiki/Line%E2%80%93sphere_intersection) equation and some rudamentary lighting calculations. 
I hope to expand it in the future to allow a compute shader to take a voxel point cloud and generate an image based on ray-voxel intersections.

I wrote a [blog post](https://rodesp.dev/blog/posts/AdventureswithImageFormatsinGLSLVulkanComputeShaders/) about the first hurdle I encountered during its development, which had nothing to do with Rust, and everything to do with Vulkan compute shaders.


![image](https://github.com/RodEsp/rusty-ray-tracer/assets/1084688/718f9bb2-a97e-4476-a3e5-a35f5997e798)
