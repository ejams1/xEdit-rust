{
  Parity probe (issue #25): the navigation tree's selection as the host
  functions read it -- wbSelectedFiles and wbSelectedFilesToFileNames over
  the selection DoRunScript makes (every root node), and again after JumpTo
  moved the focus to a record of the probe master.
}
unit HostSelection;

function B(aValue: Boolean): string;
begin
  if aValue then
    Result := 'True'
  else
    Result := 'False';
end;

procedure ShowSelection(aTag: string);
var
  lst: TList;
  sl: TStringList;
  i: Integer;
begin
  lst := TList.Create;
  sl := TStringList.Create;
  try
    wbSelectedFiles(lst);
    AddMessage(aTag + ' wbSelectedFiles count ' + IntToStr(lst.Count));
    for i := 0 to Pred(lst.Count) do
      AddMessage(aTag + '   ' + Name(ObjectToElement(lst[i])));
    wbSelectedFiles(lst);
    AddMessage(aTag + ' wbSelectedFiles again count ' + IntToStr(lst.Count));
    sl.Add('Skyrim.esm');
    wbSelectedFilesToFileNames(sl);
    AddMessage(aTag + ' wbSelectedFilesToFileNames count ' + IntToStr(sl.Count));
    for i := 0 to Pred(sl.Count) do
      AddMessage(aTag + '   ' + sl[i] + ' ' + B(Assigned(sl.Objects[i])));
  finally
    lst.Free;
    sl.Free;
  end;
end;

function Initialize: Integer;
var
  r: IInterface;
begin
  Result := 0;
  ShowSelection('start');
  wbSelectedFiles(nil);
  AddMessage('wbSelectedFiles(nil) returned');
  r := RecordByHexFormID('00000801');
  JumpTo(r, False);
  ShowSelection('jumped');
  JumpTo(RecordByHexFormID('00000800'), True);
  ShowSelection('jumped back');
end;

function Process(e: IInterface): Integer;
begin
  AddMessage('Process ' + Name(e));
end;

end.
