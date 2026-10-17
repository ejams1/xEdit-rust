{
  Parity probe (issue #25): the refusals of the xejviScriptAdapterHost.pas
  registrations -- each call raises inside a try/except and the class and
  message are logged.
}
unit HostErrors;

var
  sl: TStringList;
  lst: TList;

procedure Report(aName: string; aMessage: string);
begin
  AddMessage(aName + ': ' + aMessage);
end;

procedure Try1(aCase: Integer);
var
  v: Variant;
begin
  try
    case aCase of
      1: v := FileByLoadOrder('x');
      2: v := FileByLoadOrder(99);
      3: v := FileByLoadOrderFileID(5);
      4: v := RecordByHexFormID(5);
      5: v := RecordFromFileByFormID(FileByIndex(0));
      6: v := RecordFromFileByFormID(FileByIndex(0), 1, 2);
      7: v := RecordFromFileByFormID('NotLoaded.esp', $800);
      8: v := RecordFromFileByFormID(5, $800);
      9: v := IsPositionChanged(nil);
      10: v := IsPositionChanged(FileByIndex(0));
      11: v := ConflictThisForNode(nil);
      12: v := ConflictAllForNode(5);
      13: v := ConflictAllForMainRecord(nil);
      14: v := ConflictThisForMainRecord(FileByIndex(0));
      15: JumpTo(nil, False);
      16: JumpTo(FileByIndex(0), False);
      17: GenerateLODTES4(nil);
      18: GenerateLODTES5Trees(FileByIndex(0));
      19: GenerateLODTES5Objects(5);
      20: wbSelectedFilesToFileNames(nil);
      21: wbSelectedFilesToFileNames(lst);
      22: v := ConflictAllForElements(1, 2);
      23: v := RecordFromFileByFormID();
      24: v := FileByLoadOrderFileID(nil);
      25: v := AddNewFileName('a|b.esp');
      26: v := AddNewFileName('');
      27: v := RecordFromFileByFormID(FileByIndex(0), 1.5);
    end;
    if VarIsClear(v) then
      Report(IntToStr(aCase), 'no error, Unassigned')
    else if VarType(v) = varUnknown then
      Report(IntToStr(aCase), 'no error, interface')
    else
      Report(IntToStr(aCase), 'no error, ' + VarToStr(v) + ' (VarType ' + IntToStr(VarType(v)) + ')');
  except
    on E: Exception do
      Report(IntToStr(aCase), E.Message);
  end;
end;

function Initialize: Integer;
var
  i: Integer;
begin
  Result := 0;
  sl := TStringList.Create;
  lst := TList.Create;
  for i := 1 to 27 do
    Try1(i);
  sl.Free;
  lst.Free;
end;

end.
