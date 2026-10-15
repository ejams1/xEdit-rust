{ parity probe: x-038 }
unit P_x_038;

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
  i := 'abc';
  AddMessage('after');
end;
end.
