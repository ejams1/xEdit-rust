{
  Parity fixture (issue #27): the scripted filter. `FilterScripted` makes the
  filter walk call the script's `Filter` function back through the
  interpreter for every main record node it reaches; the function's result
  decides whether the node stays. The probe pins the visitation order (the
  `f<n>:` lines, one per call, in the walk's own order), acceptance and
  rejection by the EDID, an edit made during the callback (record 3 is
  renamed and the edit is saved), a filter state change during the callback
  (record 5 clears `FilterScripted`, after which the walk stops calling the
  script, and the filter clears the flag for good when it ends), and the
  node set the later `Process` walk sees (`p:` lines, the records the filter
  kept alone).
}
unit FilterScripted;

var
  gCount: Integer;

function Filter(e: IInterface): Boolean;
begin
  Inc(gCount);
  AddMessage('f' + IntToStr(gCount) + ': ' + Name(e));
  if gCount = 3 then
    SetElementEditValues(e, 'EDID', GetElementEditValues(e, 'EDID') + 'Edited');
  if gCount = 5 then
    FilterScripted := False;
  Result := Pos('Keep', GetElementEditValues(e, 'EDID')) > 0;
end;

function Initialize: Integer;
begin
  FilterConflictAll := False;
  FilterConflictThis := False;
  FilterByInjectStatus := False;
  FilterInjectStatus := False;
  FilterByNotReachableStatus := False;
  FilterNotReachableStatus := False;
  FilterByReferencesInjectedStatus := False;
  FilterReferencesInjectedStatus := False;
  FilterByEditorID := False;
  FilterEditorID := '';
  FilterByName := False;
  FilterName := '';
  FilterByVWD := False;
  FilterVWD := False;
  FilterByHasVWDMesh := False;
  FilterHasVWDMesh := False;
  FilterByHasPrecombinedMesh := False;
  FilterHasPrecombinedMesh := False;
  FilterByPersistent := False;
  FilterPersistent := False;
  FilterUnnecessaryPersistent := False;
  FilterMasterIsTemporary := False;
  FilterIsMaster := False;
  FilterPersistentPosChanged := False;
  FlattenBlocks := True;
  FlattenCellChilds := True;
  AssignPersWrldChild := False;
  InheritConflictByParent := False;
  FilterByElementValue := False;
  FilterElementValue := '';
  FilterByRegexComparison := False;
  FilterBySignature := False;
  FilterSignatures := '';
  FilterByBaseSignature := False;
  FilterBaseSignatures := '';
  FilterByBaseEditorID := False;
  FilterBaseEditorID := '';
  FilterByBaseName := False;
  FilterBaseName := '';
  FilterScaledActors := False;
  FilterScripted := True;

  ApplyFilter;
  // Not `gCount` here: the port applies a deferred filter when the call
  // returns, so a statement after `ApplyFilter` reads the script's state as
  // it was before the filter ran (upstream runs it inside the call); the
  // `final calls=` line of `Finalize` is the count both sides agree on.
  AddMessage('applied');
  Result := 0;
end;

function Process(e: IInterface): Integer;
begin
  AddMessage('p: ' + Name(e));
end;

function Finalize: Integer;
begin
  AddMessage('final calls=' + IntToStr(gCount));
end;

end.
