use std::str;

use futures::{sink::SinkExt, stream::StreamExt};
use tokio_util::codec::Decoder;

use tokio_serial::SerialPortBuilderExt;

use log::{error, info};

use camtrig::CamtrigCodec;
use camtrig::{Error, Result};
use camtrig_comms::{ChannelState, DeviceState, OnState, ToDevice};

/// this handles the serial port and therefore the interaction with the device
async fn try_serial(serial_device: &str, next_state: &DeviceState) {
    #[allow(unused_mut)]
    let mut port = tokio_serial::new(serial_device, 9600)
        .open_native_async()
        .unwrap();

    #[cfg(unix)]
    port.set_exclusive(false)
        .expect("Unable to set serial port exclusive to false");

    let (mut writer, mut reader) = CamtrigCodec::new().framed(port).split();

    let msg = ToDevice::DeviceState(*next_state);
    info!("sending: {:?}", msg);
    writer.send(msg).await.unwrap();

    let start = std::time::Instant::now();

    let printer = async move {
        while let Some(msg) = reader.next().await {
            match msg {
                Ok(camtrig_comms::FromDevice::EchoResponse8(d)) => {
                    let buf = [d.0, d.1, d.2, d.3, d.4, d.5, d.6, d.7];
                    let sent_millis: u64 = byteorder::ReadBytesExt::read_u64::<
                        byteorder::LittleEndian,
                    >(&mut std::io::Cursor::new(buf))
                    .unwrap();
                    let now = start.elapsed();
                    let now_millis: u64 =
                        (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
                    info!("round trip time: {} msec", now_millis - sent_millis);
                }
                Ok(msg) => {
                    error!("unknown message received: {:?}", msg);
                }
                Err(e) => {
                    panic!("unexpected error: {}: {:?}", e, e);
                }
            }
        }
    };
    tokio::spawn(printer);

    // Create a stream to call our closure every second.
    let mut interval_stream = tokio::time::interval(std::time::Duration::from_millis(1000));

    let stream_future = async move {
        loop {
            interval_stream.tick().await;
            // This closure is called once a second.

            let now = start.elapsed();
            let now_millis: u64 = (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
            let mut d = vec![];
            {
                use byteorder::WriteBytesExt;
                d.write_u64::<byteorder::LittleEndian>(now_millis).unwrap();
            }
            let msg = ToDevice::EchoRequest8((d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]));
            info!("sending: {:?}", msg);

            writer.send(msg).await.unwrap();
        }
    };

    stream_future.await;
}

fn make_chan(num: u8, on_state: OnState) -> ChannelState {
    let intensity = camtrig_comms::MAX_INTENSITY;
    ChannelState {
        num,
        intensity,
        on_state,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();

    let matches = clap::App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            clap::Arg::with_name("device")
                .long("device")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("all_leds_on").long("all-leds-on"))
        .arg(clap::Arg::with_name("all_leds_off").long("all-leds-off"))
        .get_matches();

    let device_name = matches
        .value_of("device")
        .ok_or(Error::CamtrigError("expected device".into()))?;

    let on_state = if matches.occurrences_of("all_leds_on") > 0 {
        OnState::ConstantOn
    } else {
        OnState::Off
    };

    let next_state = DeviceState {
        ch1: make_chan(1, on_state),
        ch2: make_chan(2, on_state),
        ch3: make_chan(3, on_state),
        ch4: make_chan(4, on_state),
    };

    try_serial(device_name, &next_state).await;

    Ok(())
}
