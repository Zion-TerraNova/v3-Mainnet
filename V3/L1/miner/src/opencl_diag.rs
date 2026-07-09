//! OpenCL Diagnostic v3.1.4 — Pinpoint hang trigger on gfx900/d20
//! Known: A1(create→write→dispatch→read→drop) passes, A2(create→write) hangs.
//! Tests: (A) multiple writes without dispatch, (B) re-write same buffer after
//! dispatch, (C) dispatch-only without prior read.
use ocl::{Buffer, Device, Platform, ProQue};
use std::time::Instant;

const KERNEL_SRC: &str = r#"
__kernel void add_one(__global uint* data, uint count) {
    uint gid = get_global_id(0);
    if (gid < count) { data[gid] = data[gid] + 1u; }
}
"#;

fn flush() { use std::io::Write; std::io::stdout().flush().ok(); }

fn main() {
    println!("=== ZION OpenCL Diag v3.1.4 (hang-trigger) ===");
    flush();

    let platform = Platform::default();
    let device = Device::list(platform, Some(ocl::flags::DeviceType::GPU))
        .expect("list GPUs").into_iter().next().expect("no GPU");
    println!("GPU: {}", device.name().unwrap_or_default());
    flush();

    let t = Instant::now();
    let pq = ProQue::builder()
        .platform(platform).device(device)
        .src(KERNEL_SRC).dims(64).build().expect("build");
    println!("[0] Kernel built {:.0}ms", t.elapsed().as_secs_f64()*1000.0);
    flush();

    // === TEST A: Multiple buffers + writes, NO dispatch ===
    println!("\n=== A: Multi-write, no dispatch ===");
    flush();
    {
        for i in 0..5u32 {
            print!("[A{}] create...", i+1);
            flush();
            let b = Buffer::<u32>::builder().queue(pq.queue().clone()).len(64).build().unwrap();
            print!("write...");
            flush();
            b.write(&vec![i; 64]).enq().unwrap();
            pq.queue().finish().unwrap();
            println!("OK");
            flush();
            drop(b);
        }
    }

    // === TEST B: One dispatch, then re-write SAME buffer ===
    println!("\n=== B: Dispatch then re-write same buffer ===");
    flush();
    {
        let buf = Buffer::<u32>::builder().queue(pq.queue().clone()).len(64).build().unwrap();
        print!("[B1] write1...");
        flush();
        buf.write(&vec![10u32; 64]).enq().unwrap();
        pq.queue().finish().unwrap();
        println!("OK");
        flush();

        print!("[B2] dispatch...");
        flush();
        let k = pq.kernel_builder("add_one").arg(&buf).arg(64u32).build().unwrap();
        unsafe { k.cmd().global_work_size(64).enq().unwrap(); }
        pq.queue().finish().unwrap();
        println!("OK");
        flush();

        print!("[B3] read...");
        flush();
        let mut res = vec![0u32; 64];
        buf.read(&mut res).enq().unwrap();
        pq.queue().finish().unwrap();
        println!("OK res[0]={}", res[0]);
        flush();

        print!("[B4] re-write same buf...");
        flush();
        buf.write(&vec![20u32; 64]).enq().unwrap();
        pq.queue().finish().unwrap();
        println!("OK");
        flush();

        print!("[B5] dispatch2...");
        flush();
        let k2 = pq.kernel_builder("add_one").arg(&buf).arg(64u32).build().unwrap();
        unsafe { k2.cmd().global_work_size(64).enq().unwrap(); }
        pq.queue().finish().unwrap();
        println!("OK");
        flush();

        print!("[B6] read2...");
        flush();
        let mut res2 = vec![0u32; 64];
        buf.read(&mut res2).enq().unwrap();
        pq.queue().finish().unwrap();
        println!("OK res[0]={}", res2[0]);
        flush();

        drop(k);
        drop(k2);
        drop(buf);
    }

    // === TEST C: Dispatch without read, then new buffer write ===
    println!("\n=== C: Dispatch (no read), then new buffer ===");
    flush();
    {
        let buf = Buffer::<u32>::builder().queue(pq.queue().clone()).len(64).build().unwrap();
        print!("[C1] write...");
        flush();
        buf.write(&vec![30u32; 64]).enq().unwrap();
        pq.queue().finish().unwrap();
        println!("OK");
        flush();

        print!("[C2] dispatch (no read)...");
        flush();
        let k = pq.kernel_builder("add_one").arg(&buf).arg(64u32).build().unwrap();
        unsafe { k.cmd().global_work_size(64).enq().unwrap(); }
        pq.queue().finish().unwrap();
        println!("OK");
        flush();

        drop(k);
        drop(buf);

        print!("[C3] new buf + write...");
        flush();
        let buf2 = Buffer::<u32>::builder().queue(pq.queue().clone()).len(64).build().unwrap();
        buf2.write(&vec![40u32; 64]).enq().unwrap();
        pq.queue().finish().unwrap();
        println!("OK");
        flush();

        drop(buf2);
    }

    // === TEST D: Mining loop simulation (repeated write→dispatch→read) ===
    println!("\n=== D: Mining loop (10 cycles, same buffer) ===");
    flush();
    {
        let buf = Buffer::<u32>::builder().queue(pq.queue().clone()).len(64).build().unwrap();
        for i in 0..10u32 {
            let data: Vec<u32> = (0..64).map(|x| x + i*100).collect();
            buf.write(&data).enq().unwrap();
            pq.queue().finish().unwrap();
            let k = pq.kernel_builder("add_one").arg(&buf).arg(64u32).build().unwrap();
            unsafe { k.cmd().global_work_size(64).enq().unwrap(); }
            pq.queue().finish().unwrap();
            let mut res = vec![0u32; 64];
            buf.read(&mut res).enq().unwrap();
            pq.queue().finish().unwrap();
            print!("[D{}] res[0]={} ", i+1, res[0]);
            flush();
            drop(k);
        }
        println!("\nD COMPLETE");
        flush();
        drop(buf);
    }

    println!("\n=== DIAGNOSTIC v3.1.4 COMPLETE ===");
}
