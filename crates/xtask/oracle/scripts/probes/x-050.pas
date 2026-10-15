{ parity probe: x-050 }
unit P_x_050;

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
  d := Double('x');
  AddMessage('after');
end;
end.
