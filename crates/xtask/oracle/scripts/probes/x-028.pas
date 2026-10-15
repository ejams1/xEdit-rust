{ parity probe: x-028 }
unit P_x_028;

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
  Inc('x');
  AddMessage('after');
end;
end.
