-- try to interpret the data field of all websockets as Signal WebSocketMessage protobufs
do
   local websocket_dissector = Dissector.get("websocket")
   local protobuf_dissector = Dissector.get("protobuf")
   local proto = Proto("signal_protobuf", "Protobufs in Signal WebSockets")
   local f_length = ProtoField.uint32("signal_protobuf.length", "Length", base.DEC)
   wsField = Field.new("websocket")
   wsDataField = Field.new("data.data")

   proto.fields = { f_length }
   proto.dissector = function(tvb, pinfo, tree)
      if wsField() ~= nil then
         local dataField = wsDataField()
         if dataField ~= nil then
            local subtree = tree:add(proto, tvb())
            pinfo.private["pb_msg_type"] = "message,signalservice.WebSocketMessage"
            local protobufTvb = ByteArray.tvb(dataField.range:tvb():bytes(), "Reassembled protobuf data")
            protobuf_dissector:call(protobufTvb, pinfo, tree)
            pinfo.columns.protocol:set('signal_protobuf')
         end
      end
   end
   DissectorTable.get("tls.port"):add(0, proto)
   register_postdissector(proto)
end

-- now that there's a WebSocketMessage protobuf to examine, bind some additional dissectors to its fields and possibly subfields
do
   local protobuf_field_table = DissectorTable.get("protobuf_field")
   local json_dissector = Dissector.get("json")
   local protobuf_dissector = Dissector.get("protobuf")

   local signal_body_proto = Proto("signal_body", "parse Signal body fields, as JSON or another protobuf")
   local signal_content_proto = Proto("signal_content", "parse Signal content protobuf fields")

   jsonMembers = Field.new("json.member_with_value")

   signal_content_proto.dissector = function(tvb, pinfo, tree)
      local protobufTvb = tvb:bytes(1, tvb:len()-1):tvb("content for protobuf")
      pinfo.private["pb_msg_type"] = "message,signal.proto.sealed_sender.UnidentifiedSenderMessage"
      protobuf_dissector:call(protobufTvb, pinfo, tree)
      pinfo.columns.protocol:set('signal_sealed_sender')
   end

   signal_body_proto.dissector = function(tvb, pinfo, tree)
      -- fixme: replace this with conditional dissectors based on whether there's a "content-type:application/json" header
      if tvb:range(0, 2):string() == "{\""
      then
         -- JSON with high probability
         json_dissector:call(tvb, pinfo, tree)
         local members = { jsonMembers() }
         for k, member in pairs(members) do
            if tostring(member):sub(1, 8) == "content:" then
               local b_b64 = ByteArray.new(tostring(member):sub(8), true)
               local b_decoded = ByteArray.base64_decode(b_b64)
               signal_content_proto.dissector(b_decoded:tvb("b64 decoded bytes"), pinfo, tree)
            end
         end
      else
         pinfo.private["pb_msg_type"] = "message,textsecure.Envelope"
         protobuf_dissector:call(tvb, pinfo, tree)
         pinfo.columns.protocol:set('signal_envelope')
      end
   end
   protobuf_field_table:add("signalservice.WebSocketRequestMessage.body", signal_body_proto)
   protobuf_field_table:add("signalservice.WebSocketResponseMessage.body", signal_body_proto)
   protobuf_field_table:add("textsecure.Envelope.content", signal_content_proto)
end
