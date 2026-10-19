{
  Parity case (issue #81): the form runner of JvInterpreterFm.pas. The
  script writes a fixture unit and its DFM next to itself and calls the
  four host functions:

  - JvInterpreterMakeForm compiles the unit and reads the DFM into a hidden
    interpreter form (caption, width and the nested button are read back);
  - JvInterpreterRunForm builds it and shows it;
  - JvInterpreterRunFormModal builds it, shows it modally and frees it --
    the run case records one answer that clicks the default button
    (ModalResult = 1), so the modal result is compared;
  - JvInterpreterRunUnit compiles the unit in a fresh interpreter and runs
    its main block, which adds a line to this run's log.

  The unit and the DFM are written by the script on both sides, so the
  files the run leaves are compared too. Without recorded answers the
  modal form stays open, which is the probe phase's `oracle-dialog` class;
  the run case carries the answer.
}
unit Jvfm;

function B(aValue: Boolean): string;
begin
  if aValue then
    Result := 'True'
  else
    Result := 'False';
end;

function Initialize: Integer;
var
  sl: TStringList;
  f: TForm;
  c: TComponent;
  r: Integer;
  v: Variant;
begin
  Result := 0;

  sl := TStringList.Create;
  try
    sl.Add('unit jvfmfixture;');
    sl.Add('');
    sl.Add('interface');
    sl.Add('');
    sl.Add('implementation');
    sl.Add('');
    sl.Add('procedure main;');
    sl.Add('begin');
    sl.Add('  AddMessage(''rununit: the unit main routine ran'');');
    sl.Add('end;');
    sl.Add('end.');
    sl.SaveToFile(ScriptsPath + 'jvfmfixture.pas');

    sl.Clear;
    sl.Add('object frmFixture: TForm');
    sl.Add('  Caption = ''Fixture Form''');
    sl.Add('  ClientWidth = 320');
    sl.Add('  object Button1: TButton');
    sl.Add('    Caption = ''OK''');
    sl.Add('    Default = True');
    sl.Add('    ModalResult = 1');
    sl.Add('  end');
    sl.Add('end');
    sl.SaveToFile(ScriptsPath + 'jvfmfixture.dfm');
  finally
    sl.Free;
  end;

  // JvInterpreterMakeForm: the unit compiles and its DFM is read into a
  // hidden interpreter form.
  f := JvInterpreterMakeForm(ScriptsPath + 'jvfmfixture.pas');
  AddMessage('makeform: ' + f.Caption + ' visible=' + B(f.Visible) + ' width=' + IntToStr(f.ClientWidth));
  c := f.FindComponent('Button1');
  if Assigned(c) then
    AddMessage('makeform: Button1 caption=' + TButton(c).Caption)
  else
    AddMessage('makeform: no Button1');
  f.Free;

  // JvInterpreterRunForm: the form is shown, and the script frees it.
  f := JvInterpreterRunForm(ScriptsPath + 'jvfmfixture.pas');
  AddMessage('runform: visible=' + B(f.Visible));
  f.Free;

  // JvInterpreterRunFormModal: the recorded answer clicks the default
  // button, whose ModalResult closes the modal loop; the runner frees the
  // form.
  r := JvInterpreterRunFormModal(ScriptsPath + 'jvfmfixture.pas');
  AddMessage('runformmodal: ' + IntToStr(r));

  // JvInterpreterRunUnit: a fresh interpreter compiles the unit and runs
  // its main block; the function's own result is unassigned.
  v := JvInterpreterRunUnit(ScriptsPath + 'jvfmfixture.pas');
  AddMessage('rununit: result type ' + IntToStr(VarType(v)));
end;

end.
