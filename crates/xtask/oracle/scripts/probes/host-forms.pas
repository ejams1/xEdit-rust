{
  Parity probe (issue #25): the two form handles of xejviScriptAdapterHost.pas
  -- frmMain (the main form) and frmFileSelect (a new TfrmFileSelect each
  call) -- read without showing them: class, name, caption, the components
  of the selection form and their design-time state.
}
unit HostForms;

function B(aValue: Boolean): string;
begin
  if aValue then
    Result := 'True'
  else
    Result := 'False';
end;

function Initialize: Integer;
var
  f, g: TForm;
  c: TComponent;
  clb: TCheckListBox;
  frm: TForm;
begin
  Result := 0;
  f := frmMain;
  AddMessage('frmMain assigned ' + B(Assigned(f)));
  AddMessage('frmMain ClassName ' + f.ClassName);
  AddMessage('frmMain Name ' + f.Name);
  AddMessage('frmMain same ' + B(frmMain = f));
  frm := TForm.Create(frmMain);
  AddMessage('owned form ClassName ' + frm.ClassName);
  frm.Free;

  g := frmFileSelect;
  AddMessage('frmFileSelect ClassName ' + g.ClassName);
  AddMessage('frmFileSelect Name ' + g.Name);
  AddMessage('frmFileSelect Caption ' + g.Caption);
  AddMessage('frmFileSelect new each call ' + B(frmFileSelect <> g));
  AddMessage('frmFileSelect Visible ' + B(g.Visible));
  c := g.FindComponent('CheckListBox1');
  AddMessage('CheckListBox1 ' + c.ClassName + ' ' + c.Name);
  clb := TCheckListBox(c);
  AddMessage('CheckListBox1 count ' + IntToStr(clb.Items.Count));
  clb.Items.Add('One.esp');
  clb.Items.Add('Two.esp');
  clb.Checked[1] := True;
  AddMessage('CheckListBox1 checked ' + B(clb.Checked[0]) + ' ' + B(clb.Checked[1]));
  c := g.FindComponent('btnOK');
  AddMessage('btnOK ' + c.ClassName + ' ' + TButton(c).Caption + ' ' + IntToStr(TButton(c).ModalResult));
  c := g.FindComponent('cbBackup');
  AddMessage('cbBackup ' + c.ClassName + ' ' + TCheckBox(c).Caption + ' checked ' + B(TCheckBox(c).Checked) + ' visible ' + B(TCheckBox(c).Visible));
  c := g.FindComponent('edSearch');
  AddMessage('edSearch ' + c.ClassName + ' [' + TEdit(c).Text + ']');
  c := g.FindComponent('Label1');
  AddMessage('Label1 ' + c.ClassName + ' ' + TLabel(c).Caption);
  c := g.FindComponent('PopupMenu1');
  AddMessage('PopupMenu1 ' + c.ClassName);
  c := g.FindComponent('SelectAll1');
  AddMessage('SelectAll1 ' + c.ClassName + ' ' + TMenuItem(c).Caption);
  AddMessage('missing ' + B(Assigned(g.FindComponent('Nothing'))));
  g.Free;
end;

end.
