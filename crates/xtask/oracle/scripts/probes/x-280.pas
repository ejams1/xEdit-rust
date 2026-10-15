{ parity probe: x-280 }
unit P_x_280;

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
  v := sl.CommaText; AddMessage('[' + v + ']');
  AddMessage('after');
end;
end.
