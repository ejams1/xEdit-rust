{ parity probe: x-190 }
unit P_x_190;

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
  v := not 'x';
  AddMessage('after');
end;
end.
