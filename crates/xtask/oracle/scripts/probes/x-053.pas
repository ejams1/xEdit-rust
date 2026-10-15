{ parity probe: x-053 }
unit P_x_053;

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
  b := Boolean('x');
  AddMessage('after');
end;
end.
