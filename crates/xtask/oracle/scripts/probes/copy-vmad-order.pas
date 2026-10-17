{
  Parity probe (issue #73): the sorted Scripts and Properties arrays of a
  Skyrim SE VMAD through a record copy. Nazeem [NPC_:00013BBF] of
  Skyrim.esm (properties WI and DeathContainer, stored in that order) is
  copied as an override into a new file without being read first, so the
  copy (wbCopyIsRunning, which makes TwbArrayDef.GetSorted answer False)
  is what builds his elements. The names (with their #n suffixes) and the
  property names of the source and of the copy are written after the copy,
  then those of the source again after the script let its reference go and
  took the record anew.
}
unit CopyVmadOrder;

procedure Show(aTag: string; aRecord: IInterface);
var
  scripts, script, props, prop: IInterface;
  i, j: Integer;
begin
  scripts := ElementByPath(aRecord, 'VMAD\Scripts');
  if not Assigned(scripts) then begin
    AddMessage(aTag + ' has no scripts');
    Exit;
  end;
  AddMessage(aTag + ' ' + Name(scripts) + ' count ' + IntToStr(ElementCount(scripts)));
  for i := 0 to Pred(ElementCount(scripts)) do begin
    script := ElementByIndex(scripts, i);
    AddMessage(aTag + ' ' + Name(script) + ' ' + GetElementEditValues(script, 'ScriptName'));
    props := ElementByName(script, 'Properties');
    for j := 0 to Pred(ElementCount(props)) do begin
      prop := ElementByIndex(props, j);
      AddMessage(aTag + '   ' + Name(prop) + ' ' + GetElementEditValues(prop, 'propertyName') + ' sortkey ' + SortKey(prop, False));
    end;
  end;
end;

function Initialize: Integer;
var
  i: Integer;
  skyrim, src, f, cp: IInterface;
begin
  Result := 0;
  skyrim := nil;
  for i := 0 to Pred(FileCount) do
    if GetFileName(FileByIndex(i)) = 'Skyrim.esm' then
      skyrim := FileByIndex(i);
  if not Assigned(skyrim) then begin
    AddMessage('Skyrim.esm is not loaded');
    Exit;
  end;
  src := RecordByFormID(skyrim, $00013BBF, False);
  f := AddNewFileName('CopyVmad73.esp', False);
  AddRequiredElementMasters(src, f, False, True);
  cp := wbCopyElementToFile(src, f, False, True);
  Show('source', src);
  Show('copy', cp);
  src := nil;
  cp := nil;
  src := RecordByFormID(skyrim, $00013BBF, False);
  Show('source again', src);
  cp := RecordByFormID(f, $00013BBF, False);
  Show('copy again', cp);
end;

end.
