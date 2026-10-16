{
  Parity fixture (issue #27): a `Filter` function that raises. The exception
  leaves the filter walk (upstream: out of `mniNavFilterApplyClick`, through
  the `ApplyFilter` host call, into `ApplyScript`'s handler), so the run
  aborts with the exception line and `Aborted:` -- no `Done: Applying
  Filter`, no `script terminated itself` -- and the plugins the script
  changed before the raise are still saved. The probe prints one line before
  the raise and one after the filter to pin where the walk stopped.
}
unit FilterScriptedError;

var
  gCount: Integer;

function Filter(e: IInterface): Boolean;
begin
  Inc(gCount);
  AddMessage('f' + IntToStr(gCount) + ': ' + Name(e));
  if gCount = 3 then
    raise Exception.Create('scripted filter raises');
  Result := True;
end;

function Initialize: Integer;
begin
  FilterConflictAll := False;
  FilterConflictThis := False;
  FilterByEditorID := False;
  FilterByName := False;
  FilterByElementValue := False;
  FilterBySignature := False;
  FilterByBaseSignature := False;
  FilterByBaseEditorID := False;
  FilterByBaseName := False;
  FilterByVWD := False;
  FilterByHasVWDMesh := False;
  FilterByHasPrecombinedMesh := False;
  FilterByPersistent := False;
  FilterByInjectStatus := False;
  FilterByNotReachableStatus := False;
  FilterByReferencesInjectedStatus := False;
  FilterScaledActors := False;
  FilterDeleted := False;
  FlattenBlocks := True;
  FlattenCellChilds := True;
  AssignPersWrldChild := False;
  InheritConflictByParent := False;
  FilterScripted := True;

  ApplyFilter;
  AddMessage('after the filter');
  Result := 0;
end;

function Process(e: IInterface): Integer;
begin
  AddMessage('p: ' + Name(e));
end;

end.
