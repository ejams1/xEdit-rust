{ parity probe: x-282 }
unit P_x_282;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  sl.Text := nil; AddMessage('[' + sl.Text + ']');
  AddMessage('after');
end;
end.
