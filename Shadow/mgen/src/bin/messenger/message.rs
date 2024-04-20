// Some utility functions for constructing common messages in the messengers.
// (Most message functionality is in the library, not this module.)

/// Construct and serialize a message from the sender to the recipients with the given number of blocks.
pub fn construct_message(
    sender: String,
    group: String,
    id: u32,
    blocks: u32,
) -> mgen::SerializedMessage {
    let size = std::cmp::max(blocks, 1) * mgen::PADDING_BLOCK_SIZE;
    let m = mgen::MessageHeader {
        sender,
        group,
        id,
        body: mgen::MessageBody::Size(std::num::NonZeroU32::new(size).unwrap()),
    };
    m.serialize()
}

pub fn construct_receipt(sender: String, recipient: String, id: u32) -> mgen::SerializedMessage {
    let m = mgen::MessageHeader {
        sender,
        group: recipient,
        id,
        body: mgen::MessageBody::Receipt,
    };
    m.serialize()
}
