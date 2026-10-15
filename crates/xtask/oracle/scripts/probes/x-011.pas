{ parity probe: x-011 }
unit P_x_011;

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
  s := 'abc'[0];
  AddMessage('after');
end;
end.
