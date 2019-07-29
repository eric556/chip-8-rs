extern crate rand;
extern crate sfml;

use sfml::graphics::*;
use sfml::system::*;
use sfml::window::*;

use rand::Rng;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::convert::From;

static chip8_fontset: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, //0
    0x20, 0x60, 0x20, 0x20, 0x70, //1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, //2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, //3
    0x90, 0x90, 0xF0, 0x10, 0x10, //4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, //5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, //6
    0xF0, 0x10, 0x20, 0x40, 0x40, //7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, //8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, //9
    0xF0, 0x90, 0xF0, 0x90, 0x90, //A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, //B
    0xF0, 0x80, 0x80, 0x80, 0xF0, //C
    0xE0, 0x90, 0x90, 0x90, 0xE0, //D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, //E
    0xF0, 0x80, 0xF0, 0x80, 0x80, //F
];

trait FromBig<T>{
    fn FromBig(other: T) -> Self;
}

impl FromBig<u16> for u8 {
    fn FromBig(other: u16) -> Self{
        return other.to_be_bytes()[1];
    }
}

struct Chip8 {
    opcode: u16,
    i: u16,
    pc: usize,

    // Memory Bank
    memory: [u8; 4096],

    // Registers
    v: [u8; 16],
    delay_timer: u8,
    sound_timer: u8,

    // Graphics Buffer
    gfx: [u8; 2048], // 64 x 32
    draw_flag: bool,
    clear_flag: bool,

    // Stack
    stack: [usize; 16],
    sp: usize,

    // keypad
    keypad: [bool; 16]
}

#[derive(Debug)]
struct Opcode {
    code: u16,
    top_bit: u8,
    bottom_bit: u8,
    x_reg_addr: u8,
    y_reg_addr: u8,
    addr: u16,
    byte: u8
}

impl From<u16> for Opcode {
    fn from(opcode: u16) -> Self{
        return Opcode{
            code: opcode,
            top_bit: u8::FromBig((opcode & 0xF000) >> 12),
            bottom_bit: u8::FromBig(opcode & 0x000F),
            x_reg_addr: u8::FromBig((opcode & 0x0F00) >> 8),
            y_reg_addr: u8::FromBig((opcode & 0x00F0) >> 4),
            addr: opcode & 0x0FFF,
            byte: u8::FromBig(opcode & 0x00FF)
        };
    }
}

impl Chip8 {
    fn emulate_cycle(&mut self) {
        self.opcode = u16::from(self.memory[self.pc]) << 8 | u16::from(self.memory[self.pc + 1]);
        let op = Opcode::from(self.opcode);
        println!("Running op {:?}", op);
        self.decode_op(op);

        if self.delay_timer > 0 {
            self.delay_timer = self.delay_timer - 1;
        }

        if self.sound_timer > 0 {
            if self.sound_timer == 1 {
                println!("BEEP");
            }
            self.sound_timer = self.sound_timer - 1;
        }
    }

    fn init(&mut self) {
        for n in 0..80 {
            self.memory[n] = chip8_fontset[n];
        }
    }

    fn dump(&self) {
        // println!("Memory:");
        // for byte in self.memory.iter() {
        //     print!("{:X}", byte);
        // }

        println!();
        let mut iter = 0;
        for register in self.v.iter() {
            print!("Register {:X}: ", iter);
            println!("{}", register);
            iter = iter + 1;
        }
        println!("Opcode: {:X}", self.opcode);

        println!("PC: {:X}", self.pc);

        println!("SP: {:X}", self.sp);
    }

    fn set_vx(&mut self, x: usize, val: u8) {
        self.v[x] = val;
        self.pc += 2;
    }

    fn return_sub(&mut self) {
        self.sp = self.sp - 1;

        // set the pc back to where the subroutine was called
        self.pc = self.stack[self.sp];

        // move past subroutine opcode
        self.pc += 2;
    }

    fn goto_sub(&mut self, address: usize) {
        self.stack[self.sp] = self.pc;
        self.sp += 1;
        self.pc = address;
    }

    fn skip_immediate(&mut self, x: usize, byte: u16, comp: &Fn(u16, u16) -> bool) {
        if comp(u16::from(self.v[x]), byte) {
            self.pc += 4;
        } else {
            self.pc += 2;
        }
    }

    fn skip_register(&mut self, x: usize, y: usize, comp: &Fn(u8, u8) -> bool) {
        if comp(self.v[x], self.v[y]) {
            self.pc += 4;
        } else {
            self.pc += 2;
        }
    }

    fn op_immediate(&mut self, x: usize, byte: u16, op: &Fn(u8, u8) -> u8) {
        self.v[x] = op(self.v[x], u8::FromBig(byte));
        self.pc += 2;
    }

    fn op_register(&mut self, x: usize, y: usize, op: &Fn(u8, u8) -> u8) {
        self.v[x] = op(self.v[x], self.v[y]);
        self.pc += 2;
    }

    fn op_register_carry(&mut self, x: usize, y: usize, op: &Fn(u16, u16) -> (u8, u16)) {
        let (carry, temp) = op(u16::from(self.v[x]), u16::from(self.v[y]));
        self.v[0xF] = carry;
        self.v[x] = u8::FromBig(temp);
        self.pc += 2;
    }

    fn clear_graphics(&mut self) {
        self.gfx = [0; 2048];
        self.pc += 2;
        self.clear_flag = true;
    }

    fn jump(&mut self, address: u16) {
        self.pc = usize::from(self.v[0]) + usize::from(address);
    }

    fn random(&mut self, x: usize, byte: u8) {
        self.v[x] = rand::thread_rng().gen_range(0, 255) & byte;
        self.pc += 2;
    }

    fn draw(&mut self, reg_x: usize, reg_y: usize, height: u16) {
        let x: u16 = u16::from(self.v[reg_x]);
        let y: u16 = u16::from(self.v[reg_y]);
        let mut pixel: u8;

        self.v[0xF] = 0; // reset V
        for row in 0..height {
            pixel = self.memory[usize::from(self.i) + usize::from(row)];
            for column in 0..8 {
                if (pixel & (0x80 >> column)) != 0 {
                    if usize::from(x + column + ((y + row) * 64)) < 2048 {
                        if self.gfx[usize::from(x + column + ((y + row) * 64))] == 1 {
                            self.v[0xF] = 1;
                        }
                    }
                    if usize::from(x + column + ((y + row) * 64)) < 2048 {
                        self.gfx[usize::from(x + column + ((y + row) * 64))] ^= 1;
                    }
                }
            }
        }

        self.draw_flag = true;
        self.pc += 2;
    }

    fn set_i(&mut self, address: u16) {
        self.i = address;
        self.pc += 2;
    }

    fn get_key(&mut self, x: u8) -> bool{
        return self.keypad[usize::from(self.v[usize::from(x)])]
    }

    fn decode_op(&mut self, opcode: Opcode) {
        match (opcode.top_bit, opcode.bottom_bit) {
            (0x0, 0x0) => {
                if opcode.code != 0 {
                    self.clear_graphics()
                    // self.pc += 2;
                }
            } // CLEAR SCREEN
            (0x0, 0xE) => self.return_sub(), // RETURN FROM SUB
            (0x0, _) => println!("CALL {:X}", opcode.addr), // CALL
            (0x1, _) => self.pc = usize::from(opcode.addr), // GOTO
            (0x2, _) => self.goto_sub(usize::from(opcode.addr)), // SUB PROGRAM
            (0x3, _) => self.skip_immediate(
                usize::from(opcode.x_reg_addr),
                u16::from(opcode.byte),
                &|reg: u16, byte: u16| -> bool {
                    return reg == byte;
                },
            ), // Vx == kk
            (0x4, _) => self.skip_immediate(
                usize::from(opcode.x_reg_addr),
                u16::from(opcode.byte),
                &|reg: u16, byte: u16| -> bool {
                    return reg != byte;
                },
            ), // Vx != kk
            (0x5, 0x0) => self.skip_register(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|reg1: u8, reg2: u8| -> bool {
                    return reg1 == reg2;
                },
            ), // Vx == Vy
            (0x6, _) => self.set_vx(
                usize::from(opcode.x_reg_addr),
                opcode.byte,
            ),
            (0x7, _) => self.op_immediate(
                usize::from(opcode.x_reg_addr),
                u16::from(opcode.byte),
                &|r: u8, l: u8| -> u8 {
                    return u8::FromBig(u16::from(r) + u16::from(l));
                },
            ),
            (0x8, 0x0) => self.op_register(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|_: u8, l: u8| -> u8 {
                    return l;
                },
            ),
            (0x8, 0x1) => self.op_register(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u8, l: u8| -> u8 {
                    return r | l;
                },
            ),
            (0x8, 0x2) => self.op_register(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u8, l: u8| -> u8 {
                    return r & l;
                },
            ),
            (0x8, 0x3) => self.op_register(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u8, l: u8| -> u8 {
                    return r ^ l;
                },
            ),
            (0x8, 0x4) => self.op_register_carry(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u16, l: u16| -> (u8, u16) {
                    let temp: u16 = r + l;
                    let mut carry: u8 = 0;
                    if temp > 255 {
                        carry = 1;
                    }
                    return (carry, temp);
                },
            ),
            (0x8, 0x5) => self.op_register_carry(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u16, l: u16| -> (u8, u16) {
                    let temp: u16 = r - l;
                    let mut carry: u8 = 1;
                    if l > r {
                        carry = 0;
                    }

                    return (carry, temp);
                },
            ),
            (0x8, 0x6) => self.op_register_carry(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u16, _: u16| -> (u8, u16) {
                    let lsb: u8 = u8::FromBig(r & 0x0001);
                    let temp: u16 = r >> 1;

                    return (lsb, temp);
                },
            ),
            (0x8, 0x7) => self.op_register_carry(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u16, l: u16| -> (u8, u16) {
                    let temp: u16 = l - r;
                    let mut carry: u8 = 1;
                    if r > l {
                        carry = 0;
                    }

                    return (carry, temp);
                },
            ),
            (0x8, 0xE) => self.op_register_carry(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|r: u16, _: u16| -> (u8, u16) {
                    let msb: u8 = u8::FromBig((r & 0xF000) >> 15);
                    let temp: u16 = r << 1;

                    return (msb, temp);
                },
            ),
            (0x9, 0x0) => self.skip_register(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                &|reg1: u8, reg2: u8| -> bool {
                    return reg1 != reg2;
                },
            ), // Vx != Vy
            (0xA, _) => self.set_i(opcode.addr), //LD I, addr
            (0xB, _) => self.jump(opcode.addr),
            (0xC, _) => self.random(
                usize::from(opcode.x_reg_addr),
                opcode.byte,
            ),
            (0xD, _) => self.draw(
                usize::from(opcode.x_reg_addr),
                usize::from(opcode.y_reg_addr),
                u16::from(opcode.bottom_bit),
            ),
            (0xE,0xE) => {
                if self.get_key(opcode.x_reg_addr) {
                    self.pc += 4;
                }    
                self.pc += 2;
            },
            (0xE, 0x1) => {
                if !self.get_key(opcode.x_reg_addr) {
                    self.pc += 4;
                }
                self.pc += 2;
            },
            (0xF, 0x3) => {
                let temp: u8 = self.v[usize::from(opcode.x_reg_addr)];
                self.memory[usize::from(self.i)] = (temp / 100) % 10;
                self.memory[usize::from(self.i + 1)] = (temp / 10) % 10;
                self.memory[usize::from(self.i + 2)] = temp % 10;            
            },
            (0xF, 0x5) => {
                match opcode.byte {
                    0x15 => {
                        self.delay_timer = self.v[usize::from(opcode.x_reg_addr)];
                        self.pc += 2;
                    },
                    0x55 => {
                        for i in 0..opcode.x_reg_addr {
                            self.memory[usize::from(self.i) + usize::from(i)] = self.v[usize::from(i)];
                        }
                        self.pc += 2;
                    },
                    0x65 => {
                        for i in 0..opcode.x_reg_addr {
                            self.v[usize::from(i)] = self.memory[usize::from(self.i) + usize::from(i)];
                        }
                        self.pc += 2;
                    },
                    _ => println!("Opcode not implemented: {:x}", opcode.code)
                }
            },
            (0xF, 0x7) => {
                self.v[usize::from(opcode.x_reg_addr)] = self.delay_timer;
                self.pc += 2;
            },
            (0xF, 0x8) => {
                self.sound_timer = self.v[usize::from(opcode.x_reg_addr)];
            },
            (0xF, 0x9) => {
                self.i = u16::from(self.v[usize::from(opcode.x_reg_addr)]) * 5;
            },
            (0xF, 0xA) => {
                let mut counter: u8 = 0;
                for key in self.keypad.iter() {
                    if *key {
                        self.v[usize::from(opcode.x_reg_addr)] = counter;
                        self.pc += 2;
                    }
                    counter += 1;
                }
            },
            (0xF, 0xE) => {
                self.i = self.i + u16::from(self.v[usize::from(opcode.x_reg_addr)]);
                self.pc += 2;
            },
            _ => println!("Opcode not implemented: {:x}", opcode.code),
        }
    }
}

fn to_rgba(gfx: [u8; 2048]) -> [u8; 8192] {
    // println!("to rgba");
    let mut temp: [u8; 8192] = [0; 8192];
    let mut counter = 0;
    for byte in gfx.iter() {
        let mut color: u8 = 0x00;
        if *byte == 0x1 {
            color = 0xFF;
        }

        temp[counter] = color;
        temp[counter + 1] = color;
        temp[counter + 2] = color;
        temp[counter + 3] = color;

        counter += 4;
    }

    return temp;
}

fn main() {
    let mut _cp8: Chip8 = Chip8 {
        opcode: 0,
        i: 0,
        pc: 0x200,
        delay_timer: 0,
        sound_timer: 0,
        sp: 0,
        draw_flag: false,
        clear_flag: true,
        memory: [0; 4096],
        v: [0; 16],
        gfx: [0; 2048],
        stack: [0; 16],
        keypad: [false; 16]
    };

    let args: Vec<String> = env::args().collect();
    println!("{:?}", args);

    let file = File::open(&args[1]);
    let mut rom = Vec::new();
    file.unwrap().read_to_end(&mut rom);

    _cp8.init();

    let mut counter = 0;
    for byte in rom.iter() {
        _cp8.memory[_cp8.pc + counter] = *byte;
        counter += 1;
    }

    // Create the window of the application
    let context_settings = ContextSettings {
        ..Default::default()
    };

    let mut window = RenderWindow::new(
        (64 * 8, 32 * 8),
        "Chip 8",
        Style::DEFAULT,
        &context_settings,
    );
    window.set_vertical_sync_enabled(true);
    window.set_key_repeat_enabled(false);

    let mut texture = Texture::new(64, 32).unwrap();

    loop {
        while let Some(ev) = window.poll_event() {
            match ev {
                Event::Closed => return,
                _ => {}
            }
        }

        _cp8.emulate_cycle();

        if _cp8.clear_flag {
            _cp8.clear_flag = false;
            window.clear(&Color::BLACK);
            println!("CLEAR");
            window.display();
        }

        if _cp8.draw_flag {
            texture.update_from_pixels(&to_rgba(_cp8.gfx), 64, 32, 0, 0);
            let mut temp_sprite = Sprite::with_texture(&texture);
            temp_sprite.set_position((0.0, 0.0));
            temp_sprite.set_scale((8.0, 8.0));
            _cp8.draw_flag = false;
            window.clear(&Color::BLACK);
            window.draw(&temp_sprite);
            window.display();
        }
    }
}
