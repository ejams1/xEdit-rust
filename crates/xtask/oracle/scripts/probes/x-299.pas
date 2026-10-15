{ parity probe: x-299 }
unit P_x_299;

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
  sl.Sorted := True; sl.Add('b'); sl.Add('A'); AddMessage(sl.Text);
  AddMessage('after');
end;
end.
