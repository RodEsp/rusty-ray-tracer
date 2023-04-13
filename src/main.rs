extern crate sdl2; 

use sdl2::pixels::Color;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::rect::{Point, Rect};

use std::time::Duration;
 
const IMAGE_WIDTH: u32 = 256 * 4;
const IMAGE_HEIGHT: u32 = 256 * 3;

pub fn main() {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let ttf_context = sdl2::ttf::init().unwrap();
 
    let window = video_subsystem.window("Rusty Ray Tracer", IMAGE_WIDTH, IMAGE_HEIGHT)
        .position_centered()
        .build()
        .unwrap();
 
    let mut canvas = window.into_canvas().present_vsync().build().unwrap();
    let texture_creator = canvas.texture_creator();

 
    let mut font = ttf_context.load_font("./FiraCode-Regular.ttf", 128).expect("Failed to load font");
    font.set_style(sdl2::ttf::FontStyle::NORMAL);

    let target = Rect::new(10,10,IMAGE_WIDTH/6,IMAGE_HEIGHT/24);

    let surface = font.render("Hello World!").solid(Color::RGB(200, 200, 200)).map_err(|e| e.to_string()).expect("Failed to render text");
    let texture = texture_creator.create_texture_from_surface(&surface).map_err(|e| e.to_string()).expect("Failed to create texture");


    // Calculate pixel colors
    for x in 0..IMAGE_WIDTH {
        for y in 0..IMAGE_HEIGHT {
            let r = (((x as f32 / ((IMAGE_WIDTH-1) as f32))) * 255.99) as u8;
            let g = (((y as f32 / ((IMAGE_HEIGHT-1) as f32))) * 255.99) as u8;
            let b: u8 = 64;
            canvas.set_draw_color(Color::RGB(r, g, b));
            canvas.draw_point(Point::new(x.try_into().unwrap(), y.try_into().unwrap())).unwrap();
        }   
    }

    canvas.copy(&texture, None, Some(target)).expect("Failed to copy texture to render target");
    canvas.present();

    let mut event_pump = sdl_context.event_pump().unwrap();

    // Keep the window alive until we press ESC
    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    break 'running
                },
                _ => {}
            }
        }
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}