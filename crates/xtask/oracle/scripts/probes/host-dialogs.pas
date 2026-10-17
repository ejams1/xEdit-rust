{
  Parity probe (issue #25): the host functions that ask -- AddNewFile's
  file name, AddNewFileName's "exists already" message,
  AddRequiredElementMasters' confirmation and the frmFileSelect selection
  form -- over the host-25 fixture, answered from the case's recorded
  answers; then the files it made are filled with overrides and saved when
  the GUI closes: IsPositionChanged and the conflict helpers over them.
}
unit HostDialogs;

function B(aValue: Boolean): string;
begin
  if aValue then
    Result := 'True'
  else
    Result := 'False';
end;

procedure ShowFile(aTag: string; f: IInterface);
var
  i: Integer;
begin
  if not Assigned(f) then begin
    AddMessage(aTag + ' nil');
    Exit;
  end;
  AddMessage(aTag + ' ' + Name(f) + ' masters ' + IntToStr(MasterCount(f)));
  for i := 0 to Pred(MasterCount(f)) do
    AddMessage(aTag + '   master ' + GetFileName(MasterByIndex(f, i)));
end;

function Initialize: Integer;
var
  f, g, ref1, ref2, ov1, ov2, cell: IInterface;
  frm: TForm;
  clb: TCheckListBox;
  i: Integer;
begin
  Result := 0;
  if not Assigned(RecordByHexFormID('00000A01')) then begin
    AddMessage('the host-25 fixture is not loaded');
    Exit;
  end;
  ref1 := RecordByHexFormID('00000A01');
  ref2 := RecordByHexFormID('00000A02');
  // Every record the case copies or names is read through its elements
  // first: upstream's quick init of a record named first (the CELL's
  // EDID and DATA lie before QuickInitLimit) loses its subrecords, an
  // upstream bug the port does not reproduce (issue #64), so both sides
  // start from the full records.
  ElementByPath(RecordByHexFormID('00000A00'), 'EDID');
  ElementByPath(RecordByHexFormID('00000900'), 'EDID');
  ElementByPath(ref1, 'EDID');
  ElementByPath(ref2, 'EDID');

  // InputQuery 'New Module File': answered "HostDialogs25.esp" (the
  // extension is dropped and .esp put back), with the game master added.
  f := AddNewFile;
  ShowFile('AddNewFile', f);
  // A name the data folder holds: the message, and no file.
  ShowFile('AddNewFileName existing', AddNewFileName('Skyrim.esm'));
  g := AddNewFileName('HostDialogs25b.esp');
  ShowFile('AddNewFileName', g);
  // The confirmation: answered No, then Yes.
  AddMessage('AddRequiredElementMasters no ' + B(AddRequiredElementMasters(ref1, g, False)));
  ShowFile('after no', g);
  AddMessage('AddRequiredElementMasters yes ' + B(AddRequiredElementMasters(ref1, g, False)));
  ShowFile('after yes', g);

  // The selection form: the loaded files listed, the second checked by the
  // answer (Down Down Space) and OK clicked.
  frm := frmFileSelect;
  try
    frm.Caption := 'Pick a file';
    clb := TCheckListBox(frm.FindComponent('CheckListBox1'));
    for i := 0 to Pred(FileCount) do
      clb.Items.Add(GetFileName(FileByIndex(i)));
    AddMessage('ShowModal ' + IntToStr(frm.ShowModal));
    for i := 0 to Pred(clb.Items.Count) do
      AddMessage('  ' + clb.Items[i] + ' ' + B(clb.Checked[i]));
  finally
    frm.Free;
  end;

  // Overrides of the two references: one moved, one kept.
  ov1 := wbCopyElementToFile(ref1, f, False, True);
  ov2 := wbCopyElementToFile(ref2, f, False, True);
  SetElementEditValues(ov1, 'DATA\Position\X', '5.0');
  AddMessage('IsPositionChanged moved ' + B(IsPositionChanged(ov1)));
  AddMessage('IsPositionChanged kept ' + B(IsPositionChanged(ov2)));
  AddMessage('IsPositionChanged master ' + B(IsPositionChanged(ref1)));
  AddMessage('ConflictAllForMainRecord moved ' + IntToStr(ConflictAllForMainRecord(ov1)) + ' this ' + IntToStr(ConflictThisForMainRecord(ov1)));
  AddMessage('ConflictAllForMainRecord kept ' + IntToStr(ConflictAllForMainRecord(ov2)) + ' this ' + IntToStr(ConflictThisForMainRecord(ov2)));
  AddMessage('IsPositionChanged kept after ' + B(IsPositionChanged(ov2)));
  AddMessage('ConflictThisForMainRecord master ' + IntToStr(ConflictThisForMainRecord(ref1)));
  AddMessage('ConflictAllForElements moved ' + IntToStr(ConflictAllForElements(ref1, ov1, False, False)));
  AddMessage('ConflictAllForElements kept ' + IntToStr(ConflictAllForElements(ref2, ov2, False, False)));
  AddMessage('ConflictAllForElements position ' + IntToStr(ConflictAllForElements(ElementByPath(ref1, 'DATA'), ElementByPath(ov1, 'DATA'), False, False)));
  // An edit resets the cached status: the kept one moves too.
  SetElementEditValues(ov2, 'DATA\Position\Y', '7.0');
  AddMessage('IsPositionChanged kept moved ' + B(IsPositionChanged(ov2)));
  cell := RecordByHexFormID('00000A00');
  AddMessage('cell overridden ' + IntToStr(ConflictAllForMainRecord(cell)) + ' this ' + IntToStr(ConflictThisForMainRecord(cell)));

  // The conflict filter with the status inherited by the parents, read in
  // the next call into the script (Process).
  FilterConflictAll := True;
  FilterConflictAllSet := [caOnlyOne, caNoConflict, caConflictBenign, caOverride, caConflict, caConflictCritical];
  InheritConflictByParent := True;
  ApplyFilter;
end;

function Process(e: IInterface): Integer;
var
  i: Integer;
  f: IInterface;
begin
  for i := 0 to Pred(FileCount) do begin
    f := FileByIndex(i);
    AddMessage('filtered ' + Name(f) + ' all ' + VarToStr(ConflictAllForNode(f)) + ' this ' + VarToStr(ConflictThisForNode(f)));
  end;
  AddMessage('filtered ref ' + VarToStr(ConflictAllForNode(RecordByHexFormID('00000A01'))) + ' ' + VarToStr(ConflictThisForNode(RecordByHexFormID('00000A01'))));
  AddMessage('filtered cell ' + VarToStr(ConflictAllForNode(RecordByHexFormID('00000A00'))) + ' ' + VarToStr(ConflictThisForNode(RecordByHexFormID('00000A00'))));
  AddMessage('filtered gmst ' + VarToStr(ConflictAllForNode(RecordByHexFormID('00000800'))));
  Result := 1;
end;

end.
