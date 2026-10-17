{
  Parity probe (issue #25): the file and record lookups of
  xejviScriptAdapterHost.pas -- FileByLoadOrder, FileByLoadOrderFileID,
  RecordByHexFormID, RecordFromFileByFormID and IsPositionChanged over the
  probe master (its two GMST records 00000800 and 00000801), each with the
  arguments it refuses.
}
unit HostFiles;

procedure Show(aName: string; aValue: IInterface);
begin
  if Assigned(aValue) then
    AddMessage(aName + ' = ' + Name(aValue))
  else
    AddMessage(aName + ' = nil');
end;

procedure ShowVar(aName: string; aValue: Variant);
begin
  AddMessage(aName + ' = ' + VarToStr(aValue) + ' (VarType ' + IntToStr(VarType(aValue)) + ')');
end;

function Initialize: Integer;
var
  i: Integer;
  f: IInterface;
begin
  Result := 0;
  for i := 0 to Pred(FileCount) do
    AddMessage('file ' + IntToStr(i) + ' ' + Name(FileByIndex(i)) + ' load order ' + IntToStr(GetLoadOrder(FileByIndex(i))));
  Show('FileByLoadOrder(0)', FileByLoadOrder(0));
  Show('FileByLoadOrder(1)', FileByLoadOrder(1));
  Show('FileByLoadOrderFileID(''00'')', FileByLoadOrderFileID('00'));
  Show('FileByLoadOrderFileID(''01'')', FileByLoadOrderFileID('01'));
  Show('FileByLoadOrderFileID(''0'')', FileByLoadOrderFileID('0'));
  Show('FileByLoadOrderFileID(''FF'')', FileByLoadOrderFileID('FF'));
  ShowVar('VarType FileByLoadOrderFileID(''FF'')', FileByLoadOrderFileID('FF'));

  Show('RecordByHexFormID(''00000800'')', RecordByHexFormID('00000800'));
  Show('RecordByHexFormID(''00000801'')', RecordByHexFormID('00000801'));
  Show('RecordByHexFormID(''800'')', RecordByHexFormID('800'));
  Show('RecordByHexFormID(''01000800'')', RecordByHexFormID('01000800'));
  Show('RecordByHexFormID(''00000999'')', RecordByHexFormID('00000999'));
  ShowVar('VarType RecordByHexFormID(''01000800'')', RecordByHexFormID('01000800'));

  f := FileByIndex(0);
  Show('RecordFromFileByFormID(file, $800)', RecordFromFileByFormID(f, $800));
  Show('RecordFromFileByFormID(file, $FF000801)', RecordFromFileByFormID(f, $FF000801));
  Show('RecordFromFileByFormID(file, ''00000801'')', RecordFromFileByFormID(f, '00000801'));
  Show('RecordFromFileByFormID(name, $800)', RecordFromFileByFormID(GetFileName(f), $800));
  Show('RecordFromFileByFormID(file, $999)', RecordFromFileByFormID(f, $999));

  ShowVar('IsPositionChanged', IsPositionChanged(RecordByHexFormID('00000800')));
end;

end.
