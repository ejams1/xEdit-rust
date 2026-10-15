{ parity probe: x-281 }
unit P_x_281;

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
  sl.Text := 5; AddMessage('[' + sl.Text + ']');
  AddMessage('after');
end;
end.
