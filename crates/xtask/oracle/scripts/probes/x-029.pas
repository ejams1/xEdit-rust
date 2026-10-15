{ parity probe: x-029 }
unit P_x_029;

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
  Inc(i, 'x');
  AddMessage('after');
end;
end.
