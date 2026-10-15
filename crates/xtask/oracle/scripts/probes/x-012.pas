{ parity probe: x-012 }
unit P_x_012;

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
  s := 'abc'[4];
  AddMessage('after');
end;
end.
