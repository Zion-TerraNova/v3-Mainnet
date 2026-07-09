use ocl::{flags::DeviceType, Buffer, Device, Platform, ProQue};

const KERNEL_SRC: &str = r#"
__kernel void add_one(__global uint* data, uint count) {
    uint gid = get_global_id(0);
    if (gid < count) {
        data[gid] = data[gid] + 1u;
    }
}
"#;

fn flush() {
    use std::io::Write;
    std::io::stdout().flush().ok();
}

fn main() {
    const LARGE_LEN: usize = 8 * 1024 * 1024;

    println!("=== ZION OpenCL Smoke v1 ===");
    flush();

    let platforms = Platform::list();
    println!("platforms={}", platforms.len());
    flush();
    for (index, platform) in platforms.iter().enumerate() {
        println!("[P{}] {}", index, platform.name().unwrap_or_default());
    }
    flush();

    let platform = platforms.into_iter().next().expect("no OpenCL platform");
    let devices = Device::list(platform, Some(DeviceType::GPU)).expect("list GPU devices");
    println!("gpu_devices={}", devices.len());
    flush();

    let device = devices.into_iter().next().expect("no GPU device");
    println!("gpu={}", device.name().unwrap_or_default());
    flush();

    let pro_que = ProQue::builder()
        .platform(platform)
        .device(device)
        .src(KERNEL_SRC)
        .dims(64)
        .build()
        .expect("build ProQue");
    println!("[0] proque ok");
    flush();

    let buf1 = Buffer::<u32>::builder()
        .queue(pro_que.queue().clone())
        .len(64)
        .build()
        .expect("create buf1");
    println!("[1] buf1 create ok");
    flush();

    buf1.write(&vec![1u32; 64]).enq().expect("write buf1");
    pro_que.queue().finish().expect("finish buf1 write");
    println!("[2] buf1 write ok");
    flush();

    let kernel = pro_que
        .kernel_builder("add_one")
        .arg(&buf1)
        .arg(64u32)
        .build()
        .expect("build kernel");
    unsafe {
        kernel
            .cmd()
            .global_work_size(64)
            .enq()
            .expect("dispatch kernel");
    }
    pro_que.queue().finish().expect("finish kernel");
    println!("[3] buf1 dispatch ok");
    flush();

    let mut out1 = vec![0u32; 64];
    buf1.read(&mut out1).enq().expect("read buf1");
    pro_que.queue().finish().expect("finish buf1 read");
    println!("[4] buf1 read ok res0={}", out1[0]);
    flush();

    let buf2 = Buffer::<u32>::builder()
        .queue(pro_que.queue().clone())
        .len(64)
        .build()
        .expect("create buf2");
    println!("[5] buf2 create ok");
    flush();

    buf2.write(&vec![7u32; 64]).enq().expect("write buf2");
    pro_que.queue().finish().expect("finish buf2 write");
    println!("[6] buf2 write ok");
    flush();

    let mut out2 = vec![0u32; 64];
    buf2.read(&mut out2).enq().expect("read buf2");
    pro_que.queue().finish().expect("finish buf2 read");
    println!("[7] buf2 read ok res0={}", out2[0]);
    flush();

    buf1.write(&vec![9u32; 64]).enq().expect("rewrite buf1");
    pro_que.queue().finish().expect("finish buf1 rewrite");
    println!("[8] buf1 rewrite ok");
    flush();

    for cycle in 0..16u32 {
        let data = vec![cycle + 100; 64];
        buf1.write(&data).enq().expect("loop write buf1");
        pro_que.queue().finish().expect("finish loop write");

        let loop_kernel = pro_que
            .kernel_builder("add_one")
            .arg(&buf1)
            .arg(64u32)
            .build()
            .expect("build loop kernel");
        unsafe {
            loop_kernel
                .cmd()
                .global_work_size(64)
                .enq()
                .expect("dispatch loop kernel");
        }
        pro_que.queue().finish().expect("finish loop kernel");

        let mut loop_out = vec![0u32; 64];
        buf1.read(&mut loop_out).enq().expect("loop read buf1");
        pro_que.queue().finish().expect("finish loop read");
        println!("[L{}] ok res0={}", cycle + 1, loop_out[0]);
        flush();
    }

    let large = Buffer::<u32>::builder()
        .queue(pro_que.queue().clone())
        .len(LARGE_LEN)
        .build()
        .expect("create large buffer");
    println!("[G1] large buffer create ok len={}", LARGE_LEN);
    flush();

    let large_data = vec![3u32; LARGE_LEN];
    large.write(&large_data).enq().expect("write large buffer");
    pro_que.queue().finish().expect("finish large write");
    println!("[G2] large write ok");
    flush();

    let large_kernel = pro_que
        .kernel_builder("add_one")
        .arg(&large)
        .arg(LARGE_LEN as u32)
        .build()
        .expect("build large kernel");
    unsafe {
        large_kernel
            .cmd()
            .global_work_size(LARGE_LEN)
            .enq()
            .expect("dispatch large kernel");
    }
    pro_que.queue().finish().expect("finish large kernel");
    println!("[G3] large dispatch ok");
    flush();

    let mut large_out = vec![0u32; LARGE_LEN];
    large.read(&mut large_out).enq().expect("read large buffer");
    pro_que.queue().finish().expect("finish large read");
    println!("[G4] large read ok first={} last={}", large_out[0], large_out[LARGE_LEN - 1]);
    flush();

    large.write(&vec![5u32; LARGE_LEN]).enq().expect("rewrite large buffer");
    pro_que.queue().finish().expect("finish large rewrite");
    println!("[G5] large rewrite ok");
    flush();

    println!("=== SMOKE PASS ===");
    flush();
}