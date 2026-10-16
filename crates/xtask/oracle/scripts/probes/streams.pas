{
  Parity fixture (issue #30): the JSON stream adapters and the binary stream
  classes of the Classes unit.

  TJsonObject.SaveToStream and LoadFromStream over a TMemoryStream, a
  TBytesStream, a TStringStream and a TFileStream; the stream position each
  call leaves behind; the default UTF-8 form (no mark), the TEncoding.Unicode
  preamble and the byte order marks a load detects; the over-write of a
  positioned stream; an empty stream; a TBinaryReader/TBinaryWriter round
  trip of every typed member over a file stream, the writer's buffer, the
  reader's end-of-stream error; and the exception a nil stream raises. The
  files written into the run folder are compared byte for byte by the harness
  and the message log line by line. A load that fails is compared by its
  exception class (`EJsonParserException`), and the one case the port's
  direct parser reports with the RTL's own text (`utf16-load`) by its
  message too; the oracle's other messages name its tokenizer's tokens
  (`Expected "{" but found identifier (1, 2)` for the mid-stream load,
  `Unexpected end of file where "{" was expected (1, 1)` for the empty
  stream, `Expected "{" but found invalid symbol (1, 2)` for the two
  named-encoding loads), which the port's parser approximates and the log
  does not compare (see the batch's divergences). The strings written are ASCII: a `#233`
  escape is one byte in the port's byte strings and the character U+00E9 in
  the GUI's UTF-16 ones, so the writer's encoding of a non-ASCII character
  (the GUI's is UTF-8, two bytes for U+00E9; the port's lone byte is not
  valid UTF-8) is recorded as a divergence rather than compared.
}
unit StreamsProbe;

var
  gOut: string;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

procedure JsonMemory;
var
  obj, back: TJsonObject;
  ms: TMemoryStream;
begin
  obj := TJsonObject.Create;
  obj.S['name'] := 'xEdit';
  obj.I['count'] := 42;
  obj.B['flag'] := True;
  obj.F['pi'] := 3.5;
  obj.A['list'].Add(1);
  obj.A['list'].Add('two');
  obj.O['sub'].S['k'] := 'v';
  ms := TMemoryStream.Create;
  Log('memory', 'fresh size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position));
  obj.SaveToStream(ms);
  Log('memory', 'saved size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position));
  ms.SaveToFile(gOut + 'streams-memory.json');
  ms.Position := 0;
  back := TJsonObject.Create;
  back.LoadFromStream(ms);
  Log('memory', 'loaded pos=' + IntToStr(ms.Position) + ' name=' + back.S['name'] + ' count=' + IntToStr(back.I['count']));
  Log('memory', 'json=' + back.ToJSON(True));
  // A second save appends at the end (SaveToStream writes at the position).
  back.SaveToStream(ms);
  Log('memory', 'appended size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position));
  ms.SaveToFile(gOut + 'streams-appended.json');
  // A save from a position in the middle over-writes the bytes there.
  ms.Position := 3;
  obj.SaveToStream(ms);
  Log('memory', 'overwrote size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position));
  ms.SaveToFile(gOut + 'streams-overwrite.json');
  ms.Position := 2;
  try
    back.LoadFromStream(ms);
    Log('memory', 'mid-load ok name=' + back.S['name']);
  except on E: Exception do
    Log('memory', 'mid-load exc=' + E.ClassName);
  end;
  obj.Free;
  back.Free;
  ms.Free;
end;

procedure JsonEmptyStream;
var
  obj: TJsonObject;
  ms: TMemoryStream;
begin
  obj := TJsonObject.Create;
  obj.S['name'] := 'xEdit';
  ms := TMemoryStream.Create;
  try
    obj.LoadFromStream(ms);
    Log('empty', 'ok json=' + obj.ToJSON(True) + ' types=' + IntToStr(obj.Types['name']));
  except on E: Exception do
    Log('empty', 'exc=' + E.ClassName);
  end;
  obj.Free;
  ms.Free;
end;

procedure JsonEncodings;
var
  obj, back: TJsonObject;
  ms: TMemoryStream;
begin
  obj := TJsonObject.Create;
  obj.S['name'] := 'xEdit';
  // Default: UTF-8 without a mark.
  ms := TMemoryStream.Create;
  obj.SaveToStream(ms);
  Log('encoding', 'utf8 size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position));
  ms.SaveToFile(gOut + 'streams-utf8.json');
  // Utf8WithoutBOM off with a UTF-8 encoding writes the mark.
  ms.Size := 0;
  ms.Position := 0;
  obj.SaveToStream(ms, True, TEncoding.UTF8, False);
  Log('encoding', 'utf8-bom size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position));
  ms.SaveToFile(gOut + 'streams-utf8bom.json');
  // Unicode: the FF FE preamble and UTF-16 little endian text.
  ms.Size := 0;
  ms.Position := 0;
  obj.SaveToStream(ms, True, TEncoding.Unicode, True);
  Log('encoding', 'utf16 size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position));
  ms.SaveToFile(gOut + 'streams-utf16.json');
  // The load detects the mark when no encoding is given.
  ms.Position := 0;
  back := TJsonObject.Create;
  try
    back.LoadFromStream(ms);
    Log('encoding', 'utf16-load pos=' + IntToStr(ms.Position) + ' name=' + back.S['name']);
  except on E: Exception do
    Log('encoding', 'utf16-load exc=' + E.ClassName + ' ' + E.Message);
  end;
  back.Free;
  // The same file with the encoding named.
  ms.Position := 0;
  back := TJsonObject.Create;
  try
    back.LoadFromStream(ms, TEncoding.Unicode);
    Log('encoding', 'utf16-enc-load pos=' + IntToStr(ms.Position) + ' name=' + back.S['name']);
  except on E: Exception do
    Log('encoding', 'utf16-enc-load exc=' + E.ClassName);
  end;
  back.Free;
  // A named encoding wins over the mark: the mark is read as text.
  ms.Position := 0;
  back := TJsonObject.Create;
  try
    back.LoadFromStream(ms, TEncoding.UTF8);
    Log('encoding', 'utf16-as-utf8 ok name=' + back.S['name']);
  except on E: Exception do
    Log('encoding', 'utf16-as-utf8 exc=' + E.ClassName);
  end;
  back.Free;
  ms.Free;
  obj.Free;
end;

procedure JsonBytesStream;
var
  obj, back: TJsonObject;
  ms: TMemoryStream;
  r: TBinaryReader;
  bs: TBytesStream;
  fs: TFileStream;
begin
  obj := TJsonObject.Create;
  obj.S['name'] := 'xEdit';
  obj.I['count'] := 42;
  fs := TFileStream.Create(gOut + 'streams-bytes.json', fmCreate);
  try
    obj.SaveToStream(fs);
    Log('bytes', 'file size=' + IntToStr(fs.Size) + ' pos=' + IntToStr(fs.Position));
  finally
    fs.Free;
  end;
  // A TMemoryStream of the file, then a TBytesStream of those bytes read
  // back through a TBinaryReader (the LODSettings idiom).
  ms := TMemoryStream.Create;
  ms.LoadFromFile(gOut + 'streams-bytes.json');
  fs := TFileStream.Create(gOut + 'streams-bytes.json', fmOpenRead);
  r := TBinaryReader.Create(fs);
  try
    bs := TBytesStream.Create(r.ReadBytes(fs.Size));
    try
      Log('bytes', 'tbytes size=' + IntToStr(bs.Size) + ' pos=' + IntToStr(bs.Position));
      back := TJsonObject.Create;
      back.LoadFromStream(bs);
      Log('bytes', 'load pos=' + IntToStr(bs.Position) + ' name=' + back.S['name'] + ' count=' + IntToStr(back.I['count']));
      Log('bytes', 'json=' + back.ToJSON(True));
      back.Free;
    finally
      bs.Free;
    end;
  finally
    r.Free;
    fs.Free;
  end;
  Log('bytes', 'memory size=' + IntToStr(ms.Size));
  ms.Free;
  obj.Free;
end;

procedure JsonStringStream;
var
  obj, back: TJsonObject;
  ss: TStringStream;
begin
  obj := TJsonObject.Create;
  obj.S['name'] := 'xEdit';
  ss := TStringStream.Create('');
  obj.SaveToStream(ss);
  Log('string', 'size=' + IntToStr(ss.Size) + ' pos=' + IntToStr(ss.Position) + ' data=' + ss.DataString);
  ss.Position := 0;
  back := TJsonObject.Create;
  back.LoadFromStream(ss);
  Log('string', 'load pos=' + IntToStr(ss.Position) + ' json=' + back.ToJSON(True));
  back.Free;
  ss.Free;
  obj.Free;
end;

procedure BinaryWriterReader;
var
  fs: TFileStream;
  w: TBinaryWriter;
  r: TBinaryReader;
  ms: TMemoryStream;
  s: string;
begin
  fs := TFileStream.Create(gOut + 'streams-bin.dat', fmCreate);
  w := TBinaryWriter.Create(fs);
  Log('binary', 'empty size=' + IntToStr(fs.Size) + ' pos=' + IntToStr(fs.Position));
  w.Write(42);
  Log('binary', 'int size=' + IntToStr(fs.Size));
  w.Write(-7);
  w.Write(1.5);
  w.Write(True);
  try
    w.Write('abc');
    Log('binary', 'string ok size=' + IntToStr(fs.Size));
  except on E: Exception do
    Log('binary', 'write-string exc=' + E.ClassName);
  end;
  w.WriteSingle(2.5);
  Log('binary', 'buffered size=' + IntToStr(fs.Size));
  s := '';
  while Length(s) < 5000 do
    s := s + '0123456789';
  w.Write(s);
  Log('binary', 'long size=' + IntToStr(fs.Size));
  w.Free;
  Log('binary', 'freed size=' + IntToStr(fs.Size) + ' pos=' + IntToStr(fs.Position));
  fs.Free;
  // Read it back.
  fs := TFileStream.Create(gOut + 'streams-bin.dat', fmOpenRead);
  r := TBinaryReader.Create(fs);
  // `Read` at the stream's own start (the bytes of 42), then from the
  // second value: the member's shape is pinned by both. The value is read
  // twice per line (once for `IntToStr`, once for `VarType`), which the
  // compared positions show.
  try
    Log('binary', 'read-plain=' + IntToStr(r.Read) + ' type=' + IntToStr(VarType(r.Read)) + ' pos=' + IntToStr(fs.Position));
  except on E: Exception do
    Log('binary', 'read-plain exc=' + E.ClassName + ' ' + E.Message + ' pos=' + IntToStr(fs.Position));
  end;
  try
    Log('binary', 'read-plain2=' + IntToStr(r.Read) + ' type=' + IntToStr(VarType(r.Read)) + ' pos=' + IntToStr(fs.Position));
  except on E: Exception do
    Log('binary', 'read-plain2 exc=' + E.ClassName + ' ' + E.Message + ' pos=' + IntToStr(fs.Position));
  end;
  fs.Position := 0;
  Log('binary', 'read-int=' + IntToStr(r.ReadInteger) + ' pos=' + IntToStr(fs.Position));
  try
    Log('binary', 'read-int2=' + IntToStr(r.Read) + ' pos=' + IntToStr(fs.Position));
  except on E: Exception do
    Log('binary', 'read-int2 exc=' + E.ClassName + ' ' + E.Message + ' pos=' + IntToStr(fs.Position));
  end;
  Log('binary', 'read-double=' + FloatToStr(r.ReadDouble) + ' pos=' + IntToStr(fs.Position));
  try
    Log('binary', 'read-bool=' + IntToStr(r.ReadBoolean));
  except on E: Exception do
    Log('binary', 'read-bool exc=' + E.ClassName);
  end;
  try
    Log('binary', 'read-string=' + r.ReadString + ' pos=' + IntToStr(fs.Position));
  except on E: Exception do
    Log('binary', 'read-string exc=' + E.ClassName);
  end;
  Log('binary', 'read-single=' + FloatToStr(r.ReadSingle) + ' pos=' + IntToStr(fs.Position));
  Log('binary', 'read-byte=' + IntToStr(r.ReadByte) + ' pos=' + IntToStr(fs.Position));
  Log('binary', 'read-shortint=' + IntToStr(r.ReadShortInt));
  Log('binary', 'read-smallint=' + IntToStr(r.ReadSmallInt));
  Log('binary', 'read-uint16=' + IntToStr(r.ReadUInt16));
  Log('binary', 'read-uint32=' + IntToStr(r.ReadUInt32));
  Log('binary', 'read-char=' + r.ReadChar);
  Log('binary', 'end pos=' + IntToStr(fs.Position) + ' size=' + IntToStr(fs.Size));
  try
    Log('binary', 'past-end=' + IntToStr(r.ReadInteger));
  except on E: Exception do
    Log('binary', 'past-end exc=' + E.ClassName + ' ' + E.Message);
  end;
  r.Free;
  fs.Free;
  // The writer's buffer reached the file at its Free.
  ms := TMemoryStream.Create;
  ms.LoadFromFile(gOut + 'streams-bin.dat');
  Log('binary', 'size=' + IntToStr(ms.Size));
  ms.Free;
end;

procedure BinaryFlush;
var
  fs: TFileStream;
  w: TBinaryWriter;
  s: string;
begin
  fs := TFileStream.Create(gOut + 'streams-flush.dat', fmCreate);
  w := TBinaryWriter.Create(fs);
  w.Write('abcd');
  Log('flush', 'after4 size=' + IntToStr(fs.Size) + ' pos=' + IntToStr(fs.Position));
  s := '';
  while Length(s) < 4092 do
    s := s + 'x';
  w.Write(s);
  Log('flush', 'after4096 size=' + IntToStr(fs.Size) + ' pos=' + IntToStr(fs.Position));
  w.Write('z');
  Log('flush', 'after4097 size=' + IntToStr(fs.Size) + ' pos=' + IntToStr(fs.Position));
  s := '';
  while Length(s) < 8192 do
    s := s + 'y';
  w.Write(s);
  Log('flush', 'after8192 size=' + IntToStr(fs.Size) + ' pos=' + IntToStr(fs.Position));
  w.Free;
  Log('flush', 'freed size=' + IntToStr(fs.Size));
  fs.Free;
end;

procedure NilAndFreed;
var
  obj: TJsonObject;
  ms: TMemoryStream;
  r: TBinaryReader;
  w: TBinaryWriter;
begin
  obj := TJsonObject.Create;
  try
    obj.LoadFromStream(nil);
    Log('nil', 'load ok');
  except on E: Exception do
    Log('nil', 'load exc=' + E.ClassName);
  end;
  try
    obj.SaveToStream(nil);
    Log('nil', 'save ok');
  except on E: Exception do
    Log('nil', 'save exc=' + E.ClassName);
  end;
  try
    r := TBinaryReader.Create(nil);
    Log('nil', 'reader created');
  except on E: Exception do
    Log('nil', 'reader-create exc=' + E.ClassName);
  end;
  try
    w := TBinaryWriter.Create(nil);
    Log('nil', 'writer created');
  except on E: Exception do
    Log('nil', 'writer-create exc=' + E.ClassName);
  end;
  ms := TMemoryStream.Create;
  ms.Write('abcd', 4);
  ms.Free;
  try
    obj.LoadFromStream(ms);
    Log('freed', 'load ok json=' + obj.ToJSON(True));
  except on E: Exception do
    Log('freed', 'load exc=' + E.ClassName);
  end;
end;

function Initialize: integer;
begin
  Result := 0;
  gOut := ScriptsPath;
  JsonMemory;
  JsonEmptyStream;
  JsonEncodings;
  JsonBytesStream;
  JsonStringStream;
  BinaryWriterReader;
  BinaryFlush;
  NilAndFreed;
end;

end.
