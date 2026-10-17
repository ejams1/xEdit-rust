{
  Parity probe (issue #25): AddNewFileName over the probe master -- the new
  file's name, load order and masters, a light and a second plain module,
  the same name twice, the file list functions afterwards, and
  AddRequiredElementMasters, wbCopyElementToFile and the conflict status of
  the copy.
}
unit HostNewFile;

procedure ShowFile(aTag: string; f: IInterface);
var
  i: Integer;
begin
  if not Assigned(f) then begin
    AddMessage(aTag + ' nil');
    Exit;
  end;
  AddMessage(aTag + ' ' + Name(f) + ' file name ' + GetFileName(f) + ' load order ' + IntToStr(GetLoadOrder(f)) + ' masters ' + IntToStr(MasterCount(f)) + ' records ' + IntToStr(RecordCount(f)) + ' elements ' + IntToStr(ElementCount(f)));
  for i := 0 to Pred(MasterCount(f)) do
    AddMessage(aTag + '   master ' + GetFileName(MasterByIndex(f, i)));
end;

function Initialize: Integer;
var
  f, g, h, r, c: IInterface;
  i: Integer;
  v: Variant;
begin
  Result := 0;
  f := AddNewFileName('HostProbe25.esp');
  ShowFile('plain', f);
  g := AddNewFileName('HostProbe25Light.esp', True);
  ShowFile('light', g);
  h := AddNewFileName('HostProbe25Two.esm', False, False);
  ShowFile('third', h);
  ShowFile('same name', AddNewFileName('HostProbe25.esp'));
  AddMessage('FileCount ' + IntToStr(FileCount));
  for i := 0 to Pred(FileCount) do
    ShowFile('file ' + IntToStr(i), FileByIndex(i));
  ShowFile('FileByLoadOrder(1)', FileByLoadOrder(1));
  ShowFile('FileByLoadOrder(2)', FileByLoadOrder(2));
  ShowFile('FileByName', FileByName('HostProbe25Light.esp'));
  ShowFile('FileByLoadOrderFileID(01)', FileByLoadOrderFileID('01'));
  ShowFile('FileByLoadOrderFileID(FE 000)', FileByLoadOrderFileID('FE 000'));
  ShowFile('FileByLoadOrderFileID(FE000)', FileByLoadOrderFileID('FE000'));

  r := RecordByHexFormID('00000800');
  v := AddRequiredElementMasters(r, f, False, True);
  AddMessage('AddRequiredElementMasters ' + VarToStr(v));
  ShowFile('after masters', f);
  v := AddRequiredElementMasters(r, f, False, True);
  AddMessage('AddRequiredElementMasters again ' + VarToStr(v));
  c := wbCopyElementToFile(r, f, False, True);
  AddMessage('copy ' + Name(c));
  AddMessage('ConflictAllForMainRecord copy ' + IntToStr(ConflictAllForMainRecord(c)));
  AddMessage('ConflictThisForMainRecord copy ' + IntToStr(ConflictThisForMainRecord(c)));
  AddMessage('ConflictAllForMainRecord master ' + IntToStr(ConflictAllForMainRecord(r)));
  AddMessage('ConflictThisForMainRecord master ' + IntToStr(ConflictThisForMainRecord(r)));
  AddMessage('IsPositionChanged copy ' + VarToStr(IsPositionChanged(c)));
  c := wbCopyElementToFile(RecordByHexFormID('00000801'), g, True, True);
  AddMessage('new copy ' + Name(c));
  v := AddRequiredElementMasters(RecordByHexFormID('00000801'), g, True, True);
  AddMessage('AddRequiredElementMasters as new ' + VarToStr(v));
  ShowFile('light after copy', g);
  v := AddRequiredElementMasters(RecordByHexFormID('00000801'), g, False, True);
  AddMessage('AddRequiredElementMasters override ' + VarToStr(v));
  ShowFile('light after masters', g);
  v := AddRequiredElementMasters(c, f, False, True);
  AddMessage('AddRequiredElementMasters of a later file ' + VarToStr(v));
end;

end.
