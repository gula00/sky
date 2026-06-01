use anyhow::{Context, Result};
use std::{ffi::c_void, sync::mpsc, time::Duration};
use windows::{
    core::{factory, Interface},
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
                ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            },
            Dxgi::IDXGIDevice,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RoInitialize, RO_INIT_MULTITHREADED,
        },
    },
};

pub struct CapturedImage {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

pub fn capture_window(window_id: i64) -> Result<CapturedImage> {
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

    let hwnd = HWND(window_id as isize as *mut c_void);
    let (device, context, direct3d_device) = create_d3d_device()?;
    let item = create_capture_item(hwnd)?;
    let size = item.Size().context("failed to read WGC item size")?;
    if size.Width <= 0 || size.Height <= 0 {
        anyhow::bail!("WGC item has invalid bounds");
    }

    let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &direct3d_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )
    .context("failed to create WGC frame pool")?;
    let session = frame_pool
        .CreateCaptureSession(&item)
        .context("failed to create WGC capture session")?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let _ = session.SetIsBorderRequired(false);

    let (tx, rx) = mpsc::channel();
    let handler = TypedEventHandler::<Direct3D11CaptureFramePool, windows::core::IInspectable>::new(
        move |_, _| {
            let _ = tx.send(());
            Ok(())
        },
    );
    let token = frame_pool
        .FrameArrived(&handler)
        .context("failed to subscribe to WGC frame arrival")?;

    session.StartCapture().context("failed to start WGC capture")?;
    rx.recv_timeout(Duration::from_secs(2))
        .context("timed out waiting for WGC frame")?;

    let frame = frame_pool
        .TryGetNextFrame()
        .context("failed to read WGC frame")?;
    let image = frame_to_rgba(&device, &context, &frame)?;

    let _ = frame.Close();
    let _ = frame_pool.RemoveFrameArrived(token);
    let _ = session.Close();
    let _ = frame_pool.Close();
    Ok(image)
}

fn create_d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .context("failed to create D3D11 device")?;
    }

    let device = device.context("D3D11CreateDevice returned no device")?;
    let context = context.context("D3D11CreateDevice returned no immediate context")?;
    let dxgi_device: IDXGIDevice = device.cast().context("failed to cast D3D11 device to DXGI")?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
        .context("failed to create WinRT Direct3D device")?;
    let direct3d_device = inspectable
        .cast::<IDirect3DDevice>()
        .context("failed to cast WinRT Direct3D device")?;

    Ok((device, context, direct3d_device))
}

fn create_capture_item(hwnd: HWND) -> Result<GraphicsCaptureItem> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .context("failed to load WGC capture item factory")?;
    unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(hwnd) }
        .context("failed to create WGC item for window")
}

fn frame_to_rgba(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
) -> Result<CapturedImage> {
    let surface = frame.Surface().context("failed to get WGC frame surface")?;
    let access: IDirect3DDxgiInterfaceAccess = surface
        .cast()
        .context("failed to access WGC frame DXGI interface")?;
    let texture: ID3D11Texture2D = unsafe { access.GetInterface() }
        .context("failed to get WGC frame texture")?;

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
    }
    if desc.Width == 0 || desc.Height == 0 {
        anyhow::bail!("WGC frame has invalid texture bounds");
    }

    let staging_desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        ..desc
    };
    let mut staging = None;
    unsafe {
        device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .context("failed to create WGC staging texture")?;
    }
    let staging = staging.context("CreateTexture2D returned no staging texture")?;

    let src: ID3D11Resource = texture.cast().context("failed to cast source texture")?;
    let dst: ID3D11Resource = staging.cast().context("failed to cast staging texture")?;
    unsafe {
        context.CopyResource(&dst, &src);
    }

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context
            .Map(&dst, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("failed to map WGC staging texture")?;
    }

    let rgba = mapped_bgra_to_rgba(&mapped, desc.Width, desc.Height);
    unsafe {
        context.Unmap(&dst, 0);
    }

    Ok(CapturedImage {
        width: desc.Width as i32,
        height: desc.Height as i32,
        rgba,
    })
}

fn mapped_bgra_to_rgba(mapped: &D3D11_MAPPED_SUBRESOURCE, width: u32, height: u32) -> Vec<u8> {
    let row_pitch = mapped.RowPitch as usize;
    let width = width as usize;
    let height = height as usize;
    let source = unsafe {
        std::slice::from_raw_parts(mapped.pData.cast::<u8>(), row_pitch.saturating_mul(height))
    };
    let mut rgba = vec![0_u8; width * height * 4];

    for y in 0..height {
        let source_row = &source[y * row_pitch..];
        let dest_row = &mut rgba[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let source_pixel = &source_row[x * 4..x * 4 + 4];
            let dest_pixel = &mut dest_row[x * 4..x * 4 + 4];
            dest_pixel[0] = source_pixel[2];
            dest_pixel[1] = source_pixel[1];
            dest_pixel[2] = source_pixel[0];
            dest_pixel[3] = 255;
        }
    }

    rgba
}
